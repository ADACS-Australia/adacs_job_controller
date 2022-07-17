//
// Created by lewis on 2/26/20.
//

#include "../Cluster/ClusterManager.h"
#include "../DB/MySqlConnector.h"
#include "HttpUtils.h"
#include <jwt/jwt.hpp>
#include <memory>

#include "HttpServer.h"

using namespace std;

HttpServer::HttpServer(std::shared_ptr<ClusterManager> clusterManager) {
    server.config.port = 8000;
    server.config.address = "0.0.0.0";
    server.config.thread_pool_size = 32;

    // Ready the JWT token config from the environment
    auto jTokenConfig = nlohmann::json::parse(
            base64Decode(
                    GET_ENV(
                            ACCESS_SECRET_ENV_VARIABLE,
                            base64Encode("{}")
                    )
            )
    );

    // Parse the JWT json config
    for (const auto &token : jTokenConfig) {
        vJwtSecrets.emplace_back(token);
    }

    // Add the various API's
    JobApi("/job/apiv1/job/", this, clusterManager);
    FileApi("/job/apiv1/file/", this, clusterManager);
}

void HttpServer::start() {
    server_thread = thread([this]() {
        // Start server
        this->server.start();
    });

    cout << "API: Server listening on port " << server.config.port << endl << endl;
}

void HttpServer::join() {
    server_thread.join();
}

void HttpServer::stop() {
    server.stop();
    join();
}

std::unique_ptr<sAuthorizationResult> HttpServer::isAuthorized(SimpleWeb::CaseInsensitiveMultimap &headers) {
    // Get the Authorization header from the request
    auto sAuthHeader = getHeader(headers, "Authorization");

    // Check if the header existed
    if (sAuthHeader.empty())
        // Not authorized
        throw eNotAuthorized();

    // Try to decode the token with any of the valid tokens. If any token successfully decodes without setting an
    // error_code, then the user is authorized
    jwt::jwt_object decodedToken;

    for (auto &secret : vJwtSecrets) {
        // Decode the token
        std::error_code ec;
        decodedToken = jwt::decode(
                sAuthHeader,
                jwt::params::algorithms({"HS256"}),
                ec,
                jwt::params::secret(secret.secret()),
                jwt::params::verify(true)
        );

        // Check if this token was valid
        if (!ec) {
            // Everything is fine
            return std::make_unique<sAuthorizationResult>(
                    decodedToken.payload().create_json_obj(),
                    secret
            );
        }
    }

    // If the user is not authorized, raise an exception
    throw eNotAuthorized();
}