//
// Created by lewis on 3/4/20.
//

import job_status;
import settings;

#include "../Cluster/ClusterManager.h"
#include "../DB/MySqlConnector.h"
#include "../Lib/jobserver_schema.h"
#include "../Lib/Messaging/Message.h"
#include "../Lib/GeneralUtils.h"
#include "HttpServer.h"
#include "HttpUtils.h"
#include <boost/algorithm/string/classification.hpp>
#include <boost/algorithm/string/split.hpp>
#include <boost/tokenizer.hpp>
#include <chrono>
#include <cstdint>
#include <format>
#include <exception>
#include <iterator>
#include <map>
#include <memory>
#include <ratio>
#include <stdexcept>
#include <string>
#include <vector>
#include <nlohmann/json.hpp>
#include <sqlpp11/sqlpp11.h>

auto getJobs(const std::vector<uint64_t> &ids) -> nlohmann::json;

auto filterJobs(
        std::chrono::time_point<std::chrono::system_clock, std::chrono::duration<int64_t, std::ratio<1, 1>>> *startTimeGt,
        std::chrono::time_point<std::chrono::system_clock, std::chrono::duration<int64_t, std::ratio<1, 1>>> *startTimeLt,
        std::chrono::time_point<std::chrono::system_clock, std::chrono::duration<int64_t, std::ratio<1, 1>>> *endTimeGt,
        std::chrono::time_point<std::chrono::system_clock, std::chrono::duration<int64_t, std::ratio<1, 1>>> *endTimeLt,
        std::vector<uint64_t> *jobIds, std::map<std::string, uint32_t> *jobSteps,
        std::vector<std::string> &applications) -> nlohmann::json;

// NOLINTNEXTLINE(readability-function-cognitive-complexity)
void JobApi(const std::string &path, HttpServer *server, const std::shared_ptr<ClusterManager>& clusterManager) {
    // Get      -> Get job status (job id)
    // Post     -> Create new job
    // Delete   -> Delete job (job id)
    // Patch    -> Cancel job (job id)

    // Create a new job
    server->getServer().resource["^" + path + "$"]["POST"] = [clusterManager, server](
            const std::shared_ptr<HttpServerImpl::Response> &response,
            const std::shared_ptr<HttpServerImpl::Request> &request) {

        // Verify that the user is authorized
        std::unique_ptr<sAuthorizationResult> authResult;
        try {
            authResult = server->isAuthorized(request->header);
        } catch (std::exception& e) {
            dumpExceptions(e);

            // Invalid request
            response->write(SimpleWeb::StatusCode::client_error_forbidden, "Not authorized");
            return;
        }

        // TODO(lewis): Need to check parameters are valid - such as cluster

        // Create a database connection
        auto database = MySqlConnector();

        // Start a transaction
        database->start_transaction();

        try {
            // Create a new job record and submit the job
            schema::JobserverJob jobTable;
            schema::JobserverJobhistory jobHistoryTable;

            // Read the json from the post body
            nlohmann::json post_data;
            request->content >> post_data;

            // Get the cluster to submit to
            auto cluster = clusterManager->getCluster(post_data["cluster"]);
            if (!cluster) {
                // Invalid cluster
                throw std::runtime_error("Invalid cluster");
            }

            // Check that this secret has access to the specified cluster
            if (std::find(
                    authResult->secret().clusters().begin(),
                    authResult->secret().clusters().end(),
                    std::string{post_data["cluster"]}
            ) == authResult->secret().clusters().end()) {
                // Invalid cluster
                throw std::runtime_error(
                        "Application " + authResult->secret().name() + " does not have access to cluster " +
                        std::string{post_data["cluster"]}
                );
            }

            // Create the new job object
            auto jobId = database->run(
                    insert_into(jobTable)
                            .set(
                                    jobTable.user = static_cast<uint64_t>(authResult->payload()["userId"]),
                                    jobTable.parameters = std::string{post_data["parameters"]},
                                    jobTable.cluster = std::string{post_data["cluster"]},
                                    jobTable.bundle = std::string{post_data["bundle"]},
                                    jobTable.application = authResult->secret().name()
                            )
            );

            // Create the first state object
            database->run(
                    insert_into(jobHistoryTable)
                            .set(
                                    jobHistoryTable.jobId = jobId,
                                    jobHistoryTable.timestamp = std::chrono::system_clock::now(),
                                    jobHistoryTable.what = SYSTEM_SOURCE,
                                    jobHistoryTable.state = static_cast<uint32_t>(JobStatus::PENDING),
                                    jobHistoryTable.details = "Job pending"
                            )
            );

            // Tell the client to submit the job if it's online
            // If the cluster is not online - the resendMessages function in Cluster.cpp will
            // submit the job when the client comes online
            if (cluster->isOnline()) {
                // Submit the job to the cluster
                auto msg = Message(SUBMIT_JOB, Message::Priority::Medium,
                                   std::to_string(jobId) + "_" + std::string{post_data["cluster"]});
                msg.push_uint(jobId);
                msg.push_string(post_data["bundle"]);
                msg.push_string(post_data["parameters"]);
                msg.send(cluster);

                // Mark the job as submitting
                database->run(
                        insert_into(jobHistoryTable)
                                .set(
                                        jobHistoryTable.jobId = jobId,
                                        jobHistoryTable.timestamp = std::chrono::system_clock::now(),
                                        jobHistoryTable.what = SYSTEM_SOURCE,
                                        jobHistoryTable.state = static_cast<uint32_t>(JobStatus::SUBMITTING),
                                        jobHistoryTable.details = "Job submitting"
                                )
                );
            }

            // Commit the changes in the database
            database->commit_transaction();

            // Report success
            nlohmann::json result;
            result["jobId"] = jobId;

            SimpleWeb::CaseInsensitiveMultimap headers;
            headers.emplace("Content-Type", "application/json");

            response->write(SimpleWeb::StatusCode::success_ok, result.dump(), headers);
        }
        catch (std::exception& e) 
        {
            dumpExceptions(e);

            // Abort the transaction
            database->rollback_transaction(false);

            // Report bad request
            response->write(SimpleWeb::StatusCode::client_error_bad_request, "Bad request");
        }
    };

    server->getServer().resource["^" + path + "$"]["GET"] = [server](
            const std::shared_ptr<HttpServerImpl::Response> &response,
            const std::shared_ptr<HttpServerImpl::Request> &request) {

        // Query parameters (All optional)
        // jobIds:          fetch array of jobs
        // startTimeGt:     start time greater than this (Newer than this) (Integer epoch time)
        // startTimeLt:     start time less than this (Older than this) (Integer epoch time)
        // endTimeGt:       end time greater than this (Newer than this) (Integer epoch time)
        // endTimeLt:       end time less than this (Older than this) (Integer epoch time)
        // Job Step filtering (Must include a job step id and at least one filter parameter). Job step filtering filters
        // by the provided job step's MOST RECENT state.
        // jobSteps:        csv list of:-
        //      jobStepId:  the name of the job step id to filter on
        //      state:      the state of the job step

        // Job steps are combined using OR

        // So a job filter might look like
        // ?jobIDs=50,51,52&startTimeLt=1589838778&endTimeGt=1589836778&jobSteps=jid0,500,jid1,500

        // Verify that the user is authorized
        std::unique_ptr<sAuthorizationResult> authResult;
        try {
            authResult = server->isAuthorized(request->header);
        } catch (std::exception& e) {
            dumpExceptions(e);

            // Invalid request
            response->write(SimpleWeb::StatusCode::client_error_forbidden, "Not authorized");
            return;
        }

        try {
            // Process the query parameters
            auto query_fields = request->parse_query_string();

            // Parse query parameters
            auto startTimeGt = std::chrono::sys_seconds{
                    std::chrono::seconds(getQueryParamAsInt(query_fields, "startTimeGt"))
            };

            auto startTimeLt = std::chrono::sys_seconds{
                    std::chrono::seconds(getQueryParamAsInt(query_fields, "startTimeLt"))
            };

            auto endTimeGt = std::chrono::sys_seconds{
                    std::chrono::seconds(getQueryParamAsInt(query_fields, "endTimeGt"))
            };

            auto endTimeLt = std::chrono::sys_seconds{
                    std::chrono::seconds(getQueryParamAsInt(query_fields, "endTimeLt"))
            };

            auto jobIds = getQueryParamAsVectorInt(query_fields, "jobIds");
            auto sJobSteps = getQueryParamAsString(query_fields, "jobSteps");

            // Parse the the job step string to key value pairs
            std::map<std::string, unsigned int> jobSteps;
            if (hasQueryParam(query_fields, "jobSteps")) {
                std::vector<std::string> sJobStepsArray;
                auto bits = boost::split(sJobStepsArray, sJobSteps, boost::is_any_of(", "), boost::token_compress_on);

                for (unsigned int i = 0; i < sJobStepsArray.size(); i += 2) {
                    jobSteps[sJobStepsArray[i]] = stoi(sJobStepsArray[i + 1]);
                }
            }

            // Create a vector which includes the application from the secret, and any other applications it has access to
            auto applications = std::vector<std::string>({authResult->secret().name()});
            std::copy(authResult->secret().applications().begin(), authResult->secret().applications().end(),
                      std::back_inserter(applications));

            auto result = filterJobs(
                    hasQueryParam(query_fields, "startTimeGt") ? &startTimeGt : nullptr,
                    hasQueryParam(query_fields, "startTimeLt") ? &startTimeLt : nullptr,
                    hasQueryParam(query_fields, "endTimeGt") ? &endTimeGt : nullptr,
                    hasQueryParam(query_fields, "endTimeLt") ? &endTimeLt : nullptr,
                    hasQueryParam(query_fields, "jobIds") ? &jobIds : nullptr,
                    hasQueryParam(query_fields, "jobSteps") ? &jobSteps : nullptr,
                    applications
            );

            SimpleWeb::CaseInsensitiveMultimap headers;
            headers.emplace("Content-Type", "application/json");

            response->write(SimpleWeb::StatusCode::success_ok, result.dump(), headers);
        } catch (std::exception& e) {
            dumpExceptions(e);

            // Report bad request
            response->write(SimpleWeb::StatusCode::client_error_bad_request, "Bad request");
        }
    };

    server->getServer().resource["^" + path + "$"]["PATCH"] = [clusterManager, server](
            const std::shared_ptr<HttpServerImpl::Response> &response,
            const std::shared_ptr<HttpServerImpl::Request> &request) {

        // With jobId: Job to cancel

        // Verify that the user is authorized
        std::unique_ptr<sAuthorizationResult> authResult;
        try {
            authResult = server->isAuthorized(request->header);
        } catch (std::exception& e) {
            dumpExceptions(e);

            // Invalid request
            response->write(SimpleWeb::StatusCode::client_error_forbidden, "Not authorized");
            return;
        }

        // Create a database connection
        auto database = MySqlConnector();

        // Start a transaction
        database->start_transaction();

        try {
            // Get the database tables
            schema::JobserverJob jobTable;
            schema::JobserverJobhistory jobHistoryTable;

            // Read the json from the post body
            nlohmann::json post_data;
            request->content >> post_data;

            // Check if jobId is provided
            auto jobIdPtr = post_data.find("jobId");
            if (jobIdPtr == post_data.end()) {
                throw std::runtime_error("No jobId to cancel was provided");
            }

            // Get the job id
            auto jobId = static_cast<uint64_t>(jobIdPtr.value());

            // Make sure the job to cancel exists in the database
            auto jobResults =
                    database->run(
                            select(all_of(jobTable))
                                    .from(jobTable)
                                    .where(jobTable.id == jobId)
                    );

            // Check that at least one record exists for this job
            if (jobResults.empty()) {
                throw std::runtime_error("Job did not exist with the specified jobId");
            }

            const auto *const job = &jobResults.front();

            // Get the cluster for this job
            auto cluster = clusterManager->getCluster(job->cluster);
            if (!cluster) {
                throw std::runtime_error("Cluster for job did not exist");
            }

            // Check that this secret has access to the cluster for this job
            if (std::find(
                    authResult->secret().clusters().begin(),
                    authResult->secret().clusters().end(),
                    std::string{job->cluster}
            ) == authResult->secret().clusters().end()) {
                // Invalid cluster
                throw std::runtime_error(
                        "Application " + authResult->secret().name() + " does not have access to cluster " +
                        std::string{job->cluster}
                );
            }

            // Get any job histories for this job
            auto jobHistoryResults =
                    database->run(
                            select(all_of(jobHistoryTable))
                                    .from(jobHistoryTable)
                                    .where(jobHistoryTable.jobId == job->id)
                                    .order_by(jobHistoryTable.timestamp.desc())
                                    .limit(1U)
                    );


            // Check that the job is in a valid state to cancel the job
            const auto *const latestStatus = &jobHistoryResults.front();

            // Check invalid states (States where the job has finished)
            auto invalidStates = std::vector<JobStatus>{
                JobStatus::CANCELLING,
                JobStatus::CANCELLED,
                JobStatus::DELETING,
                JobStatus::DELETED,
                JobStatus::ERROR,
                JobStatus::WALL_TIME_EXCEEDED,
                JobStatus::OUT_OF_MEMORY,
                JobStatus::COMPLETED
            };
            
            if (std::find(invalidStates.begin(), invalidStates.end(), static_cast<uint32_t>(latestStatus->state)) != invalidStates.end()) {
                throw std::runtime_error("Job is in invalid state");
            }

            // If the job is pending, mark it as cancelled now since it is not yet submitted on a cluster
            if (static_cast<uint32_t>(latestStatus->state) == static_cast<uint32_t>(JobStatus::PENDING))
            {
                database->run(
                        insert_into(jobHistoryTable)
                                .set(
                                        jobHistoryTable.jobId = job->id,
                                        jobHistoryTable.timestamp = std::chrono::system_clock::now(),
                                        jobHistoryTable.what = SYSTEM_SOURCE,
                                        jobHistoryTable.state = static_cast<uint32_t>(JobStatus::CANCELLED),
                                        jobHistoryTable.details = "Job cancelled"
                                )
                );
            }
            else
            {
                // Mark the job as cancelling and message the client to cancel the job
                database->run(
                        insert_into(jobHistoryTable)
                                .set(
                                        jobHistoryTable.jobId = job->id,
                                        jobHistoryTable.timestamp = std::chrono::system_clock::now(),
                                        jobHistoryTable.what = SYSTEM_SOURCE,
                                        jobHistoryTable.state = static_cast<uint32_t>(JobStatus::CANCELLING),
                                        jobHistoryTable.details = "Job cancelling"
                                )
                );

                // Tell the client to cancel the job if it's online
                // If the cluster is not online - the resendMessages function in Cluster.cpp will
                // cancel the job when the client comes online
                if (cluster->isOnline()) {
                    // Ask the cluster to cancel the job
                    auto msg = Message(CANCEL_JOB, Message::Priority::Medium,
                                    std::to_string(job->id) + "_" + std::string{job->cluster});
                    msg.push_uint(job->id);
                    msg.send(cluster);
                }
            }

            // Commit the changes in the database
            database->commit_transaction();

            // Report success
            nlohmann::json result;
            result["cancelled"] = jobId;

            SimpleWeb::CaseInsensitiveMultimap headers;
            headers.emplace("Content-Type", "application/json");

            response->write(SimpleWeb::StatusCode::success_ok, result.dump(), headers);
        } catch (std::exception& e) {
            dumpExceptions(e);

            // Abort the transaction
            database->rollback_transaction(false);

            // Report bad request
            response->write(SimpleWeb::StatusCode::client_error_bad_request, "Bad request");
        }
    };

    server->getServer().resource["^" + path + "$"]["DELETE"] = [clusterManager, server](
            const std::shared_ptr<HttpServerImpl::Response> &response,
            const std::shared_ptr<HttpServerImpl::Request> &request) {

        // With jobId: Job to delete

        // Verify that the user is authorized
        std::unique_ptr<sAuthorizationResult> authResult;
        try {
            authResult = server->isAuthorized(request->header);
        } catch (std::exception& e) {
            dumpExceptions(e);

            // Invalid request
            response->write(SimpleWeb::StatusCode::client_error_forbidden, "Not authorized");
            return;
        }

        // Create a database connection
        auto database = MySqlConnector();

        // Start a transaction
        database->start_transaction();

        try {
            // Get the database tables
            schema::JobserverJob jobTable;
            schema::JobserverJobhistory jobHistoryTable;

            // Read the json from the post body
            nlohmann::json post_data;
            request->content >> post_data;

            // Check if jobId is provided
            auto jobIdPtr = post_data.find("jobId");
            if (jobIdPtr == post_data.end()) {
                throw std::runtime_error("No jobId to delete was provided");
            }

            // Get the job id
            auto jobId = static_cast<uint64_t>(jobIdPtr.value());

            // Make sure the job to delete exists in the database
            auto jobResults =
                    database->run(
                            select(all_of(jobTable))
                                    .from(jobTable)
                                    .where(jobTable.id == jobId)
                    );

            // Check that at least one record exists for this job
            if (jobResults.empty()) {
                throw std::runtime_error("Job did not exist with the specified jobId");
            }

            const auto *const job = &jobResults.front();

            // Get the cluster for this job
            auto cluster = clusterManager->getCluster(job->cluster);
            if (!cluster) {
                throw std::runtime_error("Cluster for job did not exist");
            }

            // Check that this secret has access to the cluster for this job
            if (std::find(
                    authResult->secret().clusters().begin(),
                    authResult->secret().clusters().end(),
                    std::string{job->cluster}
            ) == authResult->secret().clusters().end()) {
                // Invalid cluster
                throw std::runtime_error(
                        "Application " + authResult->secret().name() + " does not have access to cluster " +
                        std::string{job->cluster}
                );
            }

            // Get any job histories for this job
            auto jobHistoryResults =
                    database->run(
                            select(all_of(jobHistoryTable))
                                    .from(jobHistoryTable)
                                    .where(jobHistoryTable.jobId == job->id)
                                    .order_by(jobHistoryTable.timestamp.desc())
                                    .limit(1U)
                    );


            // Check that the job is in a valid state to delete the job
            const auto *const latestStatus = &jobHistoryResults.front();

            // Check invalid states (States where the job is running)
            auto invalidStates = std::vector<JobStatus>{
                JobStatus::SUBMITTING,
                JobStatus::SUBMITTED,
                JobStatus::QUEUED,
                JobStatus::RUNNING,
                JobStatus::CANCELLING,
                JobStatus::DELETING,
                JobStatus::DELETED
            };
            
            if (std::find(invalidStates.begin(), invalidStates.end(), static_cast<uint32_t>(latestStatus->state)) != invalidStates.end()) {
                throw std::runtime_error("Job is in invalid state");
            }

            // If the job is pending, mark it as deleted now since it is not yet submitted on a cluster
            if (static_cast<uint32_t>(latestStatus->state) == static_cast<uint32_t>(JobStatus::PENDING))
            {
                database->run(
                        insert_into(jobHistoryTable)
                                .set(
                                        jobHistoryTable.jobId = job->id,
                                        jobHistoryTable.timestamp = std::chrono::system_clock::now(),
                                        jobHistoryTable.what = SYSTEM_SOURCE,
                                        jobHistoryTable.state = static_cast<uint32_t>(JobStatus::DELETED),
                                        jobHistoryTable.details = "Job deleted"
                                )
                );
            }
            else
            {
                // Mark the job as deleting and message the client to delete the job
                database->run(
                        insert_into(jobHistoryTable)
                                .set(
                                        jobHistoryTable.jobId = job->id,
                                        jobHistoryTable.timestamp = std::chrono::system_clock::now(),
                                        jobHistoryTable.what = SYSTEM_SOURCE,
                                        jobHistoryTable.state = static_cast<uint32_t>(JobStatus::DELETING),
                                        jobHistoryTable.details = "Job deleting"
                                )
                );

                // Tell the client to delete the job if it's online
                // If the cluster is not online - the resendMessages function in Cluster.cpp will
                // delete the job when the client comes online
                if (cluster->isOnline()) {
                    // Ask the cluster to delete the job
                    auto msg = Message(DELETE_JOB, Message::Priority::Medium,
                                    std::to_string(job->id) + "_" + std::string{job->cluster});
                    msg.push_uint(job->id);
                    msg.send(cluster);
                }
            }

            // Commit the changes in the database
            database->commit_transaction();

            // Report success
            nlohmann::json result;
            result["deleted"] = jobId;

            SimpleWeb::CaseInsensitiveMultimap headers;
            headers.emplace("Content-Type", "application/json");

            response->write(SimpleWeb::StatusCode::success_ok, result.dump(), headers);
        } catch (std::exception& e) {
            dumpExceptions(e);

            // Abort the transaction
            database->rollback_transaction(false);

            // Report bad request
            response->write(SimpleWeb::StatusCode::client_error_bad_request, "Bad request");
        }
    };
}

// NOLINTBEGIN(bugprone-easily-swappable-parameters)
auto filterJobs(
        std::chrono::time_point<std::chrono::system_clock, std::chrono::duration<int64_t, std::ratio<1, 1>>> *startTimeGt,
        std::chrono::time_point<std::chrono::system_clock, std::chrono::duration<int64_t, std::ratio<1, 1>>> *startTimeLt,
        std::chrono::time_point<std::chrono::system_clock, std::chrono::duration<int64_t, std::ratio<1, 1>>> *endTimeGt,
        std::chrono::time_point<std::chrono::system_clock, std::chrono::duration<int64_t, std::ratio<1, 1>>> *endTimeLt,
        std::vector<uint64_t> *jobIds, std::map<std::string, uint32_t> *jobSteps,
        std::vector<std::string> &applications) -> nlohmann::json {
// NOLINTEND(bugprone-easily-swappable-parameters)

    // Check for valid parameters
    if ((endTimeGt != nullptr) && (endTimeLt != nullptr)) {
        throw std::runtime_error("Can't have both endTimeGt and endTimeLt");
    }

    if ((startTimeGt != nullptr) && (startTimeLt != nullptr)) {
        throw std::runtime_error("Can't have both startTimeGt and startTimeLt");
    }

    // Create a database connection
    auto database = MySqlConnector();

    // Get the tables
    schema::JobserverJob jobTable;
    schema::JobserverJobhistory jobHistoryTable;

    // Create a sub query to find the first "system" entry for each job id
    auto systemStartTimeFilterAggregate = select(
            // As crazy as this line is, for sqlpp to work, this field will not work unless it too is an aggregate.
            // min() in this case will return exactly the same thing as there is only going to be one "id" returned
            // for each of the first "system" of each job_id
            min(jobHistoryTable.id).as(jobHistoryTable.id),

            // This is what actually does the heavy lifting here to find the earliest timestamp for each job id as
            // group_by'd later
            min(jobHistoryTable.timestamp)
    )
            .from(jobHistoryTable)
                    // Filter by system job histories
            .where(jobHistoryTable.what == SYSTEM_SOURCE)
                    // For each job id
            .group_by(jobHistoryTable.jobId)
                    // Alias this for use later
            .as(sqlpp::alias::a);

    // Next create the dynamic query to handle getting jobId's that match the start time criteria
    auto systemStartTimeFilter = sqlpp::dynamic_select(*database.getDb())
            // Select only the job id
            .columns(jobHistoryTable.jobId)
            .dynamic_from(
                    // Join with the aggregate filter to filter on min timestamp for system histories
                    jobHistoryTable.join(
                            systemStartTimeFilterAggregate).on(
                            // Match the job history id with the aggregate history id
                            jobHistoryTable.id == systemStartTimeFilterAggregate.id
                    )
            )
            .dynamic_where();

    // Note that if neither start time gt or lt is provided, then the query will match all jobs, as all jobs have an
    // initial system history created at job creation time

    // If the greater than start time is provided, add the condition to the where statement
    if (startTimeGt != nullptr) {
        systemStartTimeFilter.where.add(jobHistoryTable.timestamp >= *startTimeGt);
    }

    // If the less than start time is provided add the condition to the where statement
    if (startTimeLt != nullptr) {
        systemStartTimeFilter.where.add(jobHistoryTable.timestamp <= *startTimeLt);
    }

    // Create a temporary history filter, true here as it is combined below with AND if an end time is not provided
    auto endTimeFilter = sqlpp::boolean_expression<mysql::connection>(true);
    if (endTimeGt != nullptr) {
        // Create the end time greater than comparison
        endTimeFilter = sqlpp::boolean_expression<mysql::connection>(
                jobHistoryTable.what == "_job_completion_" and jobHistoryTable.timestamp >= *endTimeGt
        );
    }

    if (endTimeLt != nullptr) {
        // endTimeLt and endTimeGt are mutually exclusive, so no problem with reassigning
        endTimeFilter = sqlpp::boolean_expression<mysql::connection>(
                jobHistoryTable.what == "_job_completion_" and jobHistoryTable.timestamp <= *endTimeLt
        );
    }

    // Next filter by job step histories
    // Create an initial value filter of false as we will compare with OR. If job steps is not supplied, the default
    // comparison should be true
    auto jobStepFilter = sqlpp::boolean_expression<mysql::connection>(jobSteps == nullptr);

    if (jobSteps != nullptr) {
        for (const auto &step : *jobSteps) {
            jobStepFilter = sqlpp::boolean_expression<mysql::connection>(
                    (jobHistoryTable.what == step.first and jobHistoryTable.state == step.second) or jobStepFilter
            );
        }
    }

    // Get all the unique job ids from the query above
    auto historyResultsFilter = sqlpp::dynamic_select(*database.getDb())
            // Select only the job id
            .columns(jobHistoryTable.jobId)
            .from(jobHistoryTable)
            .dynamic_where()
            .dynamic_group_by();

    // Set up the application filter
    historyResultsFilter.where.add(
            jobHistoryTable.jobId.in(
                    select(jobTable.id).from(jobTable).where(jobTable.application.in(sqlpp::value_list(applications)))
            )
    );

    // Set up the where filters
    historyResultsFilter.where.add(
            // Filter by the job steps
            jobStepFilter
            // Then by the end time if there is any
            and jobHistoryTable.jobId.in(
                    select(jobHistoryTable.jobId).from(jobHistoryTable).where(endTimeFilter)
            )
            // Then by the start time if there is one
            and jobHistoryTable.jobId.in(systemStartTimeFilter)
    );

    // If the user has provided a list of job id's add that to the filter
    if (jobIds != nullptr) {
        historyResultsFilter.where.add(jobHistoryTable.jobId.in(sqlpp::value_list(*jobIds)));
    }

    // Group by the job id to eliminate duplicate job ids
    historyResultsFilter.group_by.add(jobHistoryTable.jobId);

    // Get job histories that match the filter
    auto historyResults = database->run(historyResultsFilter);

    // Get all the job ids that match
    std::vector<uint64_t> filteredJobIds;
    for (const auto &history : historyResults) {
        filteredJobIds.push_back(history.jobId);
    }

    // Return the job details for the matched ids
    return getJobs(filteredJobIds);
}

auto getJobs(const std::vector<uint64_t> &ids) -> nlohmann::json {
    // Fetches multiple jobs from the database
    nlohmann::json result;

    // Create a database connection
    auto database = MySqlConnector();

    // Get the tables
    schema::JobserverJob jobTable;
    schema::JobserverJobhistory jobHistoryTable;

    // Select the jobs from the database
    auto jobResults =
            database->run(
                    select(all_of(jobTable))
                            .from(jobTable)
                            .where(jobTable.id.in(sqlpp::value_list(ids)))
            );

    auto jobHistoryResults =
            database->run(
                    select(all_of(jobHistoryTable))
                            .from(jobHistoryTable)
                            .where(jobHistoryTable.jobId.in(sqlpp::value_list(ids)))
                            .order_by(jobHistoryTable.timestamp.desc())
            );

    // Convert the histories in to json objects
    std::vector<nlohmann::json> histories;
    for (const auto &historyResult : jobHistoryResults) {
        nlohmann::json history;
        history["jobId"] = static_cast<uint64_t>(historyResult.jobId);
        history["timestamp"] = std::format("{:%F %T %Z}", historyResult.timestamp.value());
        history["what"] = historyResult.what;
        history["state"] = static_cast<uint32_t>(historyResult.state);
        history["details"] = historyResult.details;

        histories.push_back(history);
    }

    // Iterate over the jobs
    for (const auto &job : jobResults) {
        nlohmann::json jsonJob;

        // Write the job details
        jsonJob["id"] = static_cast<uint64_t>(job.id);
        jsonJob["user"] = static_cast<uint64_t>(job.user);
        jsonJob["parameters"] = job.parameters;
        jsonJob["cluster"] = job.cluster;
        jsonJob["bundle"] = job.bundle;
        jsonJob["history"] = nlohmann::json::array();

        // Write the job history
        for (auto &history : histories) {
            if (static_cast<uint64_t>(history["jobId"]) == static_cast<uint64_t>(job.id)) {
                jsonJob["history"].push_back(history);
            }
        }

        result.push_back(jsonJob);
    }

    return result;
}