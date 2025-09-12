//
// Created by lewis on 2/26/20.
//

#ifndef GWCLOUD_JOB_SERVER_HTTPSERVER_H
#define GWCLOUD_JOB_SERVER_HTTPSERVER_H

#include "../Lib/GeneralUtils.h"
#include "../Interfaces/IClusterManager.h"
#include <iostream>
#include <nlohmann/json.hpp>
#include <server_http.hpp>
#include <utility>

using HttpServerImpl = SimpleWeb::Server<SimpleWeb::HTTP>;

// Forward declarations
class ClusterManager;

class eNotAuthorized : public std::exception {
};

struct sJwtSecret {
public:
    explicit sJwtSecret(nlohmann::json jToken) {
        name_ = jToken["name"];
        secret_ = jToken["secret"];

        for (const auto &application : jToken["applications"]) {
            applications_.push_back(application);
        }

        for (const auto &cluster : jToken["clusters"]) {
            clusters_.push_back(cluster);
        }
    }

    // The name of this application
    auto name() -> const auto & { return name_; }

    // The secret (JWT Secret) for this application
    auto secret() -> const auto & { return secret_; }

    // The list of other applications that this application has access to (Jobs from these applications)
    auto applications() -> const auto & { return applications_; }

    // The list of clusters this application can submit to
    auto clusters() -> const auto & { return clusters_; }

private:
    std::string name_;
    std::string secret_;
    std::vector<std::string> applications_;
    std::vector<std::string> clusters_;
};

struct sAuthorizationResult {
public:
    sAuthorizationResult(nlohmann::json payload, sJwtSecret &secret)
            : payload_(std::move(payload)), secret_(secret) {}

    // The decoded payload from the JWT Authorization header
    auto payload() -> const auto & { return payload_; }

    // The JwtSecret that successfully decoded the Authorization header
    auto secret() -> auto & { return secret_; }

private:
    const nlohmann::json payload_;
    sJwtSecret &secret_;
};

class HttpServer {
public:
    explicit HttpServer(const std::shared_ptr<IClusterManager>& clusterManager);

    void start();

    void join();

    void stop();

    auto getServer() -> HttpServerImpl & { return this->server; }

    auto isAuthorized(SimpleWeb::CaseInsensitiveMultimap &headers) -> std::unique_ptr<sAuthorizationResult>;

private:
    HttpServerImpl server;
    std::thread server_thread;
    std::vector<sJwtSecret> vJwtSecrets;

// Testing
EXPOSE_PROPERTY_FOR_TESTING(vJwtSecrets);
};

void JobApi(const std::string &path, HttpServer *server, const std::shared_ptr<IClusterManager>& clusterManager);
void FileApi(const std::string &path, HttpServer *server, const std::shared_ptr<IClusterManager>& clusterManager);


#endif //GWCLOUD_JOB_SERVER_HTTPSERVER_H
