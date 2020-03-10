//
// Created by lewis on 3/4/20.
//
#include <exception>
#include <jwt/jwt.hpp>
#include "HttpServer.h"
#include "../DB/MySqlConnector.h"
#include "../Lib/jobserver_schema.h"
#include "../Cluster/ClusterManager.h"
#include "../Lib/JobStatus.h"

using namespace std;
using namespace schema;

std::string getHeader(const shared_ptr<HttpServerImpl::Request> &request, const std::string &header) {
    // Iterate over the headers
    for (const auto &h : request->header) {
        // Check if the header matches
        if (h.first == header) {
            // Return the header value
            return h.second;
        }
    }

    // Return an empty string
    return std::string();
}

nlohmann::json isAuthorized(const shared_ptr<HttpServerImpl::Request> &request) {
    // Get the Authorization header from the request
    auto jwt = getHeader(request, "Authorization");

    // Check if the header existed
    if (jwt.empty())
        // Not authorized
        throw exception();

    // Decode the token
    std::error_code ec;
    auto dec_obj = jwt::decode(jwt, jwt::params::algorithms({"HS256"}), ec, jwt::params::secret(JWT_SECRET),
                               jwt::params::verify(true));

    // If there is any error code, the user is not authorized
    if (ec)
        exception();

    // Everything is fine
    return dec_obj.payload().create_json_obj();
}

void JobApi(const std::string &path, HttpServerImpl *server, ClusterManager *clusterManager) {
    // Get      -> Get job status (job id)
    // Post     -> Create new job
    // Delete   -> Delete job (job id)
    // Patch    -> Cancel job (job id)

    // Create a new job
    server->resource["^" + path + "$"]["POST"] = [clusterManager](shared_ptr<HttpServerImpl::Response> response,
                                                                  shared_ptr<HttpServerImpl::Request> request) {

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
                                    jobTable.parameters = std::string(post_data["parameters"])
                            )
            );

            // Create the first state object
            db->run(
                    insert_into(jobHistoryTable)
                            .set(
                                    jobHistoryTable.jobId = jobId,
                                    jobHistoryTable.timestamp = std::chrono::system_clock::now(),
                                    jobHistoryTable.state = (uint32_t) JobStatus::SUBMITTING,
                                    jobHistoryTable.details = "Job submitting"
                            )
            );

            // Submit the job to the cluster
            auto cluster = clusterManager->getCluster(post_data["cluster"]);
            auto msg = Message(SUBMIT_JOB, Message::Priority::Medium, std::to_string(jobId) + "_" + std::string(post_data["cluster"]));
            msg.push_uint(jobId);
            msg.push_string(post_data["bundle"]);
            msg.push_string(post_data["parameters"]);
            msg.send(cluster);

            // Commit the changes in the database
            db->commit_transaction();

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

    server->resource["^/info$"]["GET"] = [clusterManager](shared_ptr<HttpServerImpl::Response> response,
                                                          shared_ptr<HttpServerImpl::Request> request) {


        stringstream stream;
        stream << "<h1>Request from " << request->remote_endpoint_address() << ":" << request->remote_endpoint_port()
               << "</h1>";

        stream << request->method << " " << request->path << " HTTP/" << request->http_version;

        stream << "<h2>Query Fields</h2>";
        auto query_fields = request->parse_query_string();
        for (auto &field : query_fields)
            stream << field.first << ": " << field.second << "<br>";

        stream << "<h2>Header Fields</h2>";
        for (auto &field : request->header)
            stream << field.first << ": " << field.second << "<br>";

        response->write(stream);
    };
}