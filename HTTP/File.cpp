//
// Created by lewis on 3/12/20.
//
#include <jwt/jwt.hpp>
#include "HttpServer.h"
#include "../DB/MySqlConnector.h"
#include "../Lib/jobserver_schema.h"
#include "../Cluster/ClusterManager.h"
#include "HttpUtils.h"

using namespace std;
using namespace schema;

void FileApi(const std::string &path, HttpServerImpl *server, ClusterManager *clusterManager) {
    // Get      -> Download file (file uuid)
    // Post     -> Create new file download
    // Delete   -> Delete file download (file uuid)

    // Create a new job
    server->resource["^" + path + "$"]["POST"] = [clusterManager](shared_ptr <HttpServerImpl::Response> response,
                                                                  shared_ptr <HttpServerImpl::Request> request) {

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
            
        } catch (...) {
            // Abort the transaction
            db->rollback_transaction(false);

            // Report bad request
            response->write(SimpleWeb::StatusCode::client_error_bad_request, "Bad request");
        }
    };
}