//
// Created by lewis on 3/4/20.
//
#include <exception>
#include <jwt/jwt.hpp>
#include <boost/algorithm/string/split.hpp>
#include <boost/algorithm/string/classification.hpp>
#include "HttpServer.h"
#include "../DB/MySqlConnector.h"
#include "../Lib/jobserver_schema.h"
#include "../Cluster/ClusterManager.h"
#include "../Lib/JobStatus.h"
#include "HttpUtils.h"
#include <sqlpp11/sqlpp11.h>
#include "date/date.h"

nlohmann::json getJob(uint32_t id);
nlohmann::json getJobs(const std::vector<uint32_t> &ids);

using namespace std;
using namespace schema;

void JobApi(const std::string &path, HttpServerImpl *server, ClusterManager *clusterManager) {
    // Get      -> Get job status (job id)
    // Post     -> Create new job
    // Delete   -> Delete job (job id)
    // Patch    -> Cancel job (job id)

    // Create a new job
    server->resource["^" + path + "$"]["POST"] = [clusterManager](shared_ptr<HttpServerImpl::Response> response,
                                                                  shared_ptr<HttpServerImpl::Request> request) {

        // Verify that the user is authorized
        nlohmann::json jwt;
        try {
            jwt = isAuthorized(request);
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

            // Create the new job object
            auto jobId = db->run(
                    insert_into(jobTable)
                            .set(
                                    jobTable.user = (uint32_t) jwt["userId"],
                                    jobTable.parameters = std::string(post_data["parameters"]),
                                    jobTable.cluster = std::string(post_data["cluster"]),
                                    jobTable.bundle = std::string(post_data["bundle"])
                            )
            );

            // Create the first state object
            db->run(
                    insert_into(jobHistoryTable)
                            .set(
                                    jobHistoryTable.jobId = jobId,
                                    jobHistoryTable.timestamp = std::chrono::system_clock::now(),
                                    jobHistoryTable.state = (uint32_t) JobStatus::PENDING,
                                    jobHistoryTable.details = "Job submitting"
                            )
            );

            // Commit the changes in the database
            db->commit_transaction();

            // Get the cluster to submit to
            auto cluster = clusterManager->getCluster(post_data["cluster"]);

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

    server->resource["^" + path + "$"]["GET"] = [clusterManager](shared_ptr<HttpServerImpl::Response> response,
                                                                 shared_ptr<HttpServerImpl::Request> request) {

        // With jobId: fetch just that job
        // With jobIdArray: fetch array of jobs

        // Verify that the user is authorized
        nlohmann::json jwt;
        try {
            jwt = isAuthorized(request);
        } catch (...) {
            // Invalid request
            response->write(SimpleWeb::StatusCode::client_error_forbidden, "Not authorized");
            return;
        }

        try {
            // Process the query parameters
            auto query_fields = request->parse_query_string();

            // Check if jobId is provided
            auto jobIdPtr = query_fields.find("jobId");
            auto jobId = 0;
            if (jobIdPtr != query_fields.end())
                jobId = std::stoi(jobIdPtr->second);

            // Check if jobIdArray is provided
            auto jobIdArrayPtr = query_fields.find("jobIdArray");
            std::vector<uint32_t> jobIdArray;
            if (jobIdArrayPtr != query_fields.end()) {
                std::vector<std::string> sJobIdArray;
                boost::split(sJobIdArray, jobIdArrayPtr->second, boost::is_any_of(", "), boost::token_compress_on);

                for (const auto &id : sJobIdArray) {
                    jobIdArray.push_back(std::stoi(id));
                }
            }

            nlohmann::json result;

            SimpleWeb::CaseInsensitiveMultimap headers;
            headers.emplace("Content-Type", "application/json");

            // Process jobId
            if (jobId) {
                // Get the job as a json object
                result = getJob(jobId);
                response->write(SimpleWeb::StatusCode::success_ok, result.dump(), headers);
                return;
            }

            // Process jobIdArray
            if (!jobIdArray.empty()) {
                // Get the jobs as an array of json object
                result = getJobs(jobIdArray);
                response->write(SimpleWeb::StatusCode::success_ok, result.dump(), headers);
                return;
            }

            // Not a valid request
            throw exception();
        } catch (...) {
            // Report bad request
            response->write(SimpleWeb::StatusCode::client_error_bad_request, "Bad request");
        }
    };

    // todo: Cancel and delete
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
        for (auto &h : jobHistoryResults) {
            if (h.jobId == job.id) {
                nlohmann::json history;
                history["timestamp"] = date::format("%F %T %Z", h.timestamp.value());
                history["state"] = (uint32_t) h.state;
                history["details"] = h.details;

                jsonJob["history"].push_back(history);
            }
        }

        result.push_back(jsonJob);
    }

    return result;
}

nlohmann::json getJob(uint32_t id) {
    // Fetches a single job from the database

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
                            .where(jobTable.id == id)
            );

    auto jobHistoryResults =
            db->run(
                    select(all_of(jobHistoryTable))
                            .from(jobHistoryTable)
                            .where(jobHistoryTable.jobId == id)
                            .order_by(jobHistoryTable.timestamp.desc())
            );


    // There's only one job, but I found I had to iterate it to get the single record, as front gave me corrupted results
    for (auto &job : jobResults) {
        // Write the job details
        result["id"] = (uint32_t) job.id;
        result["user"] = (uint32_t) job.user;
        result["parameters"] = job.parameters;
        result["cluster"] = job.cluster;
        result["bundle"] = std::string(job.bundle);
        result["history"] = nlohmann::json::array();

        // Write the job history
        for (auto &h : jobHistoryResults) {
            if (h.jobId == job.id) {
                nlohmann::json history;
                history["timestamp"] = date::format("%F %T %Z", h.timestamp.value());;
                history["state"] = (uint32_t) h.state;
                history["details"] = h.details;

                result["history"].push_back(history);
            }
        }
    }
    return result;
}
