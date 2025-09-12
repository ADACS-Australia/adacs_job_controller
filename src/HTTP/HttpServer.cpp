//
// Created by lewis on 2/26/20.
//

import settings;

#include "../Interfaces/IClusterManager.h"
#include "../Interfaces/ICluster.h"
#include "HttpServer.h"
#include "HttpUtils.h"
#include <jwt/jwt.hpp>
#include <memory>

HttpServer::HttpServer(const std::shared_ptr<IClusterManager>& clusterManager) {
    server.config.port = HTTP_PORT;
    server.config.address = "0.0.0.0";
    server.config.thread_pool_size = HTTP_WORKER_POOL_SIZE;
    server.config.timeout_content = HTTP_CONTENT_TIMEOUT_SECONDS;

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
    server_thread = std::thread([this]() {
        // Start server
        this->server.start();
    });

    std::cout << "API: Server listening on port " << server.config.port << std::endl << std::endl;
}

void HttpServer::join() {
    server_thread.join();
}

void HttpServer::stop() {
    server.stop();
    join();
}

auto HttpServer::isAuthorized(SimpleWeb::CaseInsensitiveMultimap &headers) -> std::unique_ptr<sAuthorizationResult> {
    // Get the Authorization header from the request
    auto sAuthHeader = getHeader(headers, "Authorization");

    // Check if the header existed
    if (sAuthHeader.empty()) {
        // Not authorized
        throw eNotAuthorized();
    }

    // Try to decode the token with any of the valid tokens. If any token successfully decodes without setting an
    // error_code, then the user is authorized
    jwt::jwt_object decodedToken;

    for (auto &secret : vJwtSecrets) {
        // Decode the token
        std::error_code errorCode;
        decodedToken = jwt::decode(
                sAuthHeader,
                jwt::params::algorithms({"HS256"}),
                errorCode,
                jwt::params::secret(secret.secret()),
                jwt::params::verify(true)
        );

        // Check if this token was valid
        if (!errorCode) {
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
