//
// Created by lewis on 3/4/20.
//
#include <exception>
#include "../DB/MySqlConnector.h"
#include "../Lib/jobserver_schema.h"
#include "../Cluster/ClusterManager.h"
#include "../Lib/JobStatus.h"
#include "HttpUtils.h"
#include <sqlpp11/sqlpp11.h>
#include "date/date.h"
#include <boost/algorithm/string/split.hpp>
#include <boost/algorithm/string/classification.hpp>

#include "HttpServer.h"


nlohmann::json getJobs(const std::vector<uint32_t> &ids);

nlohmann::json filterJobs(
        std::chrono::time_point<std::chrono::system_clock, std::chrono::duration<long, std::ratio<1, 1>>> *startTimeGt,
        std::chrono::time_point<std::chrono::system_clock, std::chrono::duration<long, std::ratio<1, 1>>> *startTimeLt,
        std::chrono::time_point<std::chrono::system_clock, std::chrono::duration<long, std::ratio<1, 1>>> *endTimeGt,
        std::chrono::time_point<std::chrono::system_clock, std::chrono::duration<long, std::ratio<1, 1>>> *endTimeLt,
        std::vector<unsigned long> *jobIds, std::map<std::string, unsigned int> *jobSteps,
        std::vector<std::string> &applications);

using namespace schema;

void JobApi(const std::string &path, HttpServer *server, ClusterManager *clusterManager) {
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
        } catch (...) {
            // Invalid request
            response->write(SimpleWeb::StatusCode::client_error_forbidden, "Not authorized");
            return;
        }

        // TODO: Need to check parameters are valid - such as cluster

        // Create a database connection
        auto db = MySqlConnector();

        // Start a transaction
        db->start_transaction();

        try {
            // Create a new job record and submit the job
            JobserverJob jobTable;
            JobserverJobhistory jobHistoryTable;

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
                    post_data["cluster"]
            ) == authResult->secret().clusters().end()) {
                // Invalid cluster
                throw std::runtime_error(
                        "Application " + authResult->secret().name() + " does not have access to cluster " +
                        std::string(post_data["cluster"])
                );
            }

            // Create the new job object
            auto jobId = db->run(
                    insert_into(jobTable)
                            .set(
                                    jobTable.user = (uint32_t) authResult->payload()["userId"],
                                    jobTable.parameters = std::string(post_data["parameters"]),
                                    jobTable.cluster = std::string(post_data["cluster"]),
                                    jobTable.bundle = std::string(post_data["bundle"]),
                                    jobTable.application = authResult->secret().name()
                            )
            );

            // Create the first state object
            db->run(
                    insert_into(jobHistoryTable)
                            .set(
                                    jobHistoryTable.jobId = jobId,
                                    jobHistoryTable.timestamp = std::chrono::system_clock::now(),
                                    jobHistoryTable.what = "system",
                                    jobHistoryTable.state = (uint32_t) JobStatus::PENDING,
                                    jobHistoryTable.details = "Job submitting"
                            )
            );

            // Commit the changes in the database
            db->commit_transaction();

            // Tell the client to submit the job if it's online
            // If the cluster is not online - the resendMessages function in Cluster.cpp will
            // submit the job when the client comes online
            if (cluster->isOnline()) {
                // Submit the job to the cluster
                auto msg = Message(SUBMIT_JOB, Message::Priority::Medium,
                                   std::to_string(jobId) + "_" + std::string(post_data["cluster"]));
                msg.push_uint(jobId);
                msg.push_string(post_data["bundle"]);
                msg.push_string(post_data["parameters"]);
                msg.send(cluster);
            }

            // Report success
            nlohmann::json result;
            result["jobId"] = jobId;

            SimpleWeb::CaseInsensitiveMultimap headers;
            headers.emplace("Content-Type", "application/json");

            response->write(SimpleWeb::StatusCode::success_ok, result.dump(), headers);
        }
        catch (...) {
            // Abort the transaction
            db->rollback_transaction(false);

            // Report bad request
            response->write(SimpleWeb::StatusCode::client_error_bad_request, "Bad request");
        }
    };

    server->getServer().resource["^" + path + "$"]["GET"] = [clusterManager, server](
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
        } catch (...) {
            // Invalid request
            response->write(SimpleWeb::StatusCode::client_error_forbidden, "Not authorized");
            return;
        }

        try {
            // Process the query parameters
            auto query_fields = request->parse_query_string();

            // Parse query parameters
            auto startTimeGt = date::sys_seconds{
                    std::chrono::seconds(getQueryParamAsInt(query_fields, "startTimeGt"))
            };

            auto startTimeLt = date::sys_seconds{
                    std::chrono::seconds(getQueryParamAsInt(query_fields, "startTimeLt"))
            };

            auto endTimeGt = date::sys_seconds{
                    std::chrono::seconds(getQueryParamAsInt(query_fields, "endTimeGt"))
            };

            auto endTimeLt = date::sys_seconds{
                    std::chrono::seconds(getQueryParamAsInt(query_fields, "endTimeLt"))
            };

            auto jobIds = getQueryParamAsVectorInt(query_fields, "jobIds");
            auto sJobSteps = getQueryParamAsString(query_fields, "jobSteps");

            // Parse the the job step string to key value pairs
            std::map<std::string, unsigned int> jobSteps;
            if (hasQueryParam(query_fields, "jobSteps")) {
                std::vector<std::string> sJobStepsArray;
                auto bits = boost::split(sJobStepsArray, sJobSteps, boost::is_any_of(", "), boost::token_compress_on);

                for (unsigned int i = 0; i < sJobStepsArray.size(); i += 2)
                    jobSteps[sJobStepsArray[i]] = stoi(sJobStepsArray[i + 1]);
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
        } catch (...) {
            // Report bad request
            response->write(SimpleWeb::StatusCode::client_error_bad_request, "Bad request");
        }
    };

    server->getServer().resource["^" + path + "$"]["DELETE"] = [clusterManager, server](
            const std::shared_ptr<HttpServerImpl::Response> &response,
            const std::shared_ptr<HttpServerImpl::Request> &request) {

        // todo: Not implemented yet
        return;
        // With jobId: Job to cancel

        // Verify that the user is authorized
        std::unique_ptr<sAuthorizationResult> authResult;
        try {
            authResult = server->isAuthorized(request->header);
        } catch (...) {
            // Invalid request
            response->write(SimpleWeb::StatusCode::client_error_forbidden, "Not authorized");
            return;
        }

        // Create a database connection
        auto db = MySqlConnector();

        // Start a transaction
        db->start_transaction();

        try {
            // Create a new job record and submit the job
            JobserverJob jobTable;
            JobserverJobhistory jobHistoryTable;

            // Read the json from the post body
            nlohmann::json post_data;
            request->content >> post_data;

            // Process the query parameters
            auto query_fields = request->parse_query_string();

            // Check if jobId is provided
            auto jobIdPtr = query_fields.find("jobId");
            auto jobId = 0;
            if (jobIdPtr != query_fields.end())
                jobId = std::stoi(jobIdPtr->second);

            // Check that the job id was provided
            if (!jobId)
                throw std::exception();

            auto jobHistoryResults =
                    db->run(
                            select(all_of(jobHistoryTable))
                                    .from(jobHistoryTable)
                                    .where(jobHistoryTable.jobId == jobId)
                                    .order_by(jobHistoryTable.timestamp.desc())
                    );

            // Check that at least one record exists for this job
            if (jobHistoryResults.empty())
                throw std::exception();

            // Commit the changes in the database
            db->commit_transaction();

            // Report success
            nlohmann::json result;
            result["cancelled"] = jobId;

            SimpleWeb::CaseInsensitiveMultimap headers;
            headers.emplace("Content-Type", "application/json");

            response->write(SimpleWeb::StatusCode::success_ok, result.dump(), headers);
        } catch (...) {
            // Abort the transaction
            db->rollback_transaction(false);

            // Report bad request
            response->write(SimpleWeb::StatusCode::client_error_bad_request, "Bad request");
        }
    };
}

nlohmann::json filterJobs(
        std::chrono::time_point<std::chrono::system_clock, std::chrono::duration<long, std::ratio<1, 1>>> *startTimeGt,
        std::chrono::time_point<std::chrono::system_clock, std::chrono::duration<long, std::ratio<1, 1>>> *startTimeLt,
        std::chrono::time_point<std::chrono::system_clock, std::chrono::duration<long, std::ratio<1, 1>>> *endTimeGt,
        std::chrono::time_point<std::chrono::system_clock, std::chrono::duration<long, std::ratio<1, 1>>> *endTimeLt,
        std::vector<unsigned long> *jobIds, std::map<std::string, unsigned int> *jobSteps,
        std::vector<std::string> &applications) {
    // Check for valid parameters
    if (endTimeGt && endTimeLt)
        throw std::runtime_error("Can't have both endTimeGt and endTimeLt");

    if (startTimeGt && startTimeLt)
        throw std::runtime_error("Can't have both startTimeGt and startTimeLt");

    // Create a database connection
    auto db = MySqlConnector();

    // Get the tables
    JobserverJob jobTable;
    JobserverJobhistory jobHistoryTable;

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
            .where(jobHistoryTable.what == "system")
                    // For each job id
            .group_by(jobHistoryTable.jobId)
                    // Alias this for use later
            .as(sqlpp::alias::a);

    // Next create the dynamic query to handle getting jobId's that match the start time criteria
    auto systemStartTimeFilter = sqlpp::dynamic_select(*db.getDb())
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
    if (startTimeGt)
        systemStartTimeFilter.where.add(jobHistoryTable.timestamp >= *startTimeGt);

    // If the less than start time is provided add the condition to the where statement
    if (startTimeLt)
        systemStartTimeFilter.where.add(jobHistoryTable.timestamp <= *startTimeLt);

    // Create a temporary history filter, true here as it is combined below with AND if an end time is not provided
    auto endTimeFilter = sqlpp::boolean_expression<mysql::connection>(true);
    if (endTimeGt)
        // Create the end time greater than comparison
        endTimeFilter = sqlpp::boolean_expression<mysql::connection>(
                jobHistoryTable.what == "_job_completion_" and jobHistoryTable.timestamp >= *endTimeGt
        );

    if (endTimeLt)
        // endTimeLt and endTimeGt are mutually exclusive, so no problem with reassigning
        endTimeFilter = sqlpp::boolean_expression<mysql::connection>(
                jobHistoryTable.what == "_job_completion_" and jobHistoryTable.timestamp <= *endTimeLt
        );

    // Next filter by job step histories
    // Create an initial value filter of false as we will compare with OR. If job steps is not supplied, the default
    // comparison should be true
    auto jobStepFilter = sqlpp::boolean_expression<mysql::connection>(jobSteps == nullptr);

    if (jobSteps) {
        for (const auto &s : *jobSteps) {
            jobStepFilter = sqlpp::boolean_expression<mysql::connection>(
                    (jobHistoryTable.what == s.first and jobHistoryTable.state == s.second) or jobStepFilter
            );
        }
    }

    // Get all the unique job ids from the query above
    auto historyResultsFilter = sqlpp::dynamic_select(*db.getDb())
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
    if (jobIds)
        historyResultsFilter.where.add(jobHistoryTable.jobId.in(sqlpp::value_list(*jobIds)));

    // Group by the job id to eliminate duplicate job ids
    historyResultsFilter.group_by.add(jobHistoryTable.jobId);

    // Get job histories that match the filter
    auto historyResults = db->run(historyResultsFilter);

    // Get all the job ids that match
    std::vector<uint32_t> filteredJobIds;
    for (auto &h : historyResults) {
        filteredJobIds.push_back(h.jobId);
    }

    // Return the job details for the matched ids
    return getJobs(filteredJobIds);
}

nlohmann::json getJobs(const std::vector<uint32_t> &ids) {
    // Fetches multiple jobs from the database
    nlohmann::json result;

    // Create a database connection
    auto db = MySqlConnector();

    // Get the tables
    JobserverJob jobTable;
    JobserverJobhistory jobHistoryTable;

    // Select the jobs from the database
    auto jobResults =
            db->run(
                    select(all_of(jobTable))
                            .from(jobTable)
                            .where(jobTable.id.in(sqlpp::value_list(ids)))
            );

    auto jobHistoryResults =
            db->run(
                    select(all_of(jobHistoryTable))
                            .from(jobHistoryTable)
                            .where(jobHistoryTable.jobId.in(sqlpp::value_list(ids)))
                            .order_by(jobHistoryTable.timestamp.desc())
            );

    // Convert the histories in to json objects
    std::vector<nlohmann::json> histories;
    for (auto &h : jobHistoryResults) {
        nlohmann::json history;
        history["jobId"] = (uint32_t) h.jobId;
        history["timestamp"] = date::format("%F %T %Z", h.timestamp.value());
        history["what"] = h.what;
        history["state"] = (uint32_t) h.state;
        history["details"] = h.details;

        histories.push_back(history);
    }

    // Iterate over the jobs
    for (auto &job : jobResults) {
        nlohmann::json jsonJob;

        // Write the job details
        jsonJob["id"] = (uint32_t) job.id;
        jsonJob["user"] = (uint32_t) job.user;
        jsonJob["parameters"] = job.parameters;
        jsonJob["cluster"] = job.cluster;
        jsonJob["bundle"] = job.bundle;
        jsonJob["history"] = nlohmann::json::array();

        // Write the job history
        for (auto &h : histories) {
            if ((uint32_t) h["jobId"] == job.id) {
                jsonJob["history"].push_back(h);
            }
        }

        result.push_back(jsonJob);
    }

    return result;
}
