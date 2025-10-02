//
// HttpServer C++20 module
//

module;
#include <iostream>
#include <memory>
#include <utility>

#include <jwt/jwt.hpp>
#include <nlohmann/json.hpp>
#include <server_http.hpp>

#include "../Lib/TestingMacros.h"

export module HttpServer;

import settings;
import IClusterManager;
import ICluster;
import IApplication;
import IHttpServer;
import GeneralUtils;

export using HttpServerImpl = SimpleWeb::Server<SimpleWeb::HTTP>;

export class eNotAuthorized : public std::exception
{};

export struct sJwtSecret
{
public:
    explicit sJwtSecret(nlohmann::json jToken)
    {
        // NOLINTNEXTLINE(cppcoreguidelines-pro-bounds-avoid-unchecked-container-access)
        nameValue   = jToken["name"];
        // NOLINTNEXTLINE(cppcoreguidelines-pro-bounds-avoid-unchecked-container-access)
        secretValue = jToken["secret"];

        // NOLINTNEXTLINE(cppcoreguidelines-pro-bounds-avoid-unchecked-container-access)
        for (const auto& application : jToken["applications"])
        {
            applicationsValue.push_back(application);
        }

        // NOLINTNEXTLINE(cppcoreguidelines-pro-bounds-avoid-unchecked-container-access)
        for (const auto& cluster : jToken["clusters"])
        {
            clustersValue.push_back(cluster);
        }
    }

    // The name of this application
    [[nodiscard]] auto name() const -> const auto&
    {
        return nameValue;
    }

    // The secret (JWT Secret) for this application
    [[nodiscard]] auto secret() const -> const auto&
    {
        return secretValue;
    }

    // The list of other applications that this application has access to (Jobs from these applications)
    [[nodiscard]] auto applications() const -> const auto&
    {
        return applicationsValue;
    }

    // The list of clusters this application can submit to
    [[nodiscard]] auto clusters() const -> const auto&
    {
        return clustersValue;
    }

private:
    std::string nameValue;
    std::string secretValue;
    std::vector<std::string> applicationsValue;
    std::vector<std::string> clustersValue;
};

export struct sAuthorizationResult
{
public:
    sAuthorizationResult(nlohmann::json payload, const sJwtSecret& secret)
        : payloadValue(std::move(payload)), secretValue(secret)
    {}

    // The decoded payload from the JWT Authorization header
    auto payload() -> const auto&
    {
        return payloadValue;
    }

    // The JwtSecret that successfully decoded the Authorization header
    auto secret() -> const auto&
    {
        return secretValue;
    }

private:
    // NOLINTNEXTLINE(cppcoreguidelines-avoid-const-or-ref-data-members)
    const nlohmann::json payloadValue;
    // NOLINTNEXTLINE(cppcoreguidelines-avoid-const-or-ref-data-members)
    const sJwtSecret& secretValue;
};

export class HttpServer : public IHttpServer
{
public:
    explicit HttpServer(std::shared_ptr<IApplication> app);

    void start() override;
    void stop() override;
    void join() override;
    [[nodiscard]] bool is_running() const override;

    // No interface methods needed - endpoints are registered directly in Application

    auto getServer() -> HttpServerImpl&
    {
        return this->server;
    }

    auto isAuthorized(SimpleWeb::CaseInsensitiveMultimap& headers) -> std::unique_ptr<sAuthorizationResult>;

private:
    HttpServerImpl server;
    std::thread server_thread;
    std::vector<sJwtSecret> vJwtSecrets;
    std::shared_ptr<IApplication> app;

    // Testing
    EXPOSE_PROPERTY_FOR_TESTING(vJwtSecrets);
};

// No forward declarations needed - using module imports

// Implementation
HttpServer::HttpServer(std::shared_ptr<IApplication> app) : app(std::move(app))
{
    server.config.port             = HTTP_PORT;
    server.config.address          = "0.0.0.0";
    server.config.thread_pool_size = HTTP_WORKER_POOL_SIZE;
    server.config.timeout_content  = HTTP_CONTENT_TIMEOUT_SECONDS;

    // Ready the JWT token config from the environment
    auto jTokenConfig = nlohmann::json::parse(base64Decode(GET_ENV(ACCESS_SECRET_ENV_VARIABLE, base64Encode("{}"))));

    // Load the JWT secrets
    for (const auto& jToken : jTokenConfig)
    {
        vJwtSecrets.emplace_back(jToken);
    }
}

void HttpServer::start()
{
    server_thread = std::thread([this]() {
        server.start();
    });
}

void HttpServer::join()
{
    server_thread.join();
}

void HttpServer::stop()
{
    server.stop();
    if (server_thread.joinable())
    {
        server_thread.join();
    }
}

auto HttpServer::isAuthorized(SimpleWeb::CaseInsensitiveMultimap& headers) -> std::unique_ptr<sAuthorizationResult>
{
    // Get the authorization header
    auto authHeader = headers.find("authorization");
    if (authHeader == headers.end())
    {
        throw eNotAuthorized();
    }

    auto sAuthHeader = authHeader->second;
    jwt::jwt_object decodedToken;

    for (auto& secret : vJwtSecrets)
    {
        // Decode the token
        std::error_code errorCode;
        decodedToken = jwt::decode(sAuthHeader,
                                   jwt::params::algorithms({"HS256"}),
                                   errorCode,
                                   jwt::params::secret(secret.secret()),
                                   jwt::params::verify(true));

        // Check if this token was valid
        if (!errorCode)
        {
            // Everything is fine
            return std::make_unique<sAuthorizationResult>(decodedToken.payload().create_json_obj(), secret);
        }
    }

    // If the user is not authorized, raise an exception
    throw eNotAuthorized();
}

// Interface method implementations

[[nodiscard]] bool HttpServer::is_running() const
{
    return server_thread.joinable();
}
