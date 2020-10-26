//
// Created by lewis on 2/26/20.
//

#ifndef GWCLOUD_JOB_SERVER_HTTPSERVER_H
#define GWCLOUD_JOB_SERVER_HTTPSERVER_H

#include <iostream>
#include <server_http.hpp>
#include <nlohmann/json.hpp>
#include <utility>
#include "../Lib/GeneralUtils.h"

using HttpServerImpl = SimpleWeb::Server<SimpleWeb::HTTP>;

class ClusterManager;

class eNotAuthorized : public std::exception {
};

struct sJwtSecret {
public:
    explicit sJwtSecret(nlohmann::json jToken) {
        name_ = jToken["name"];
        secret_ = jToken["secret"];

        for (const auto &n : jToken["applications"])
            applications_.push_back(n);

        for (const auto &n : jToken["clusters"])
            clusters_.push_back(n);
    }

    // The name of this application
    const auto &name() { return name_; }

    // The secret (JWT Secret) for this application
    const auto &secret() { return secret_; }

    // The list of other applications that this application has access to (Jobs from these applications)
    const auto &applications() { return applications_; }

    // The list of clusters this application can submit to
    const auto &clusters() { return clusters_; }

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
    const auto &payload() { return payload_; }

    // The JwtSecret that successfully decoded the Authorization header
    auto &secret() { return secret_; }

private:
    const nlohmann::json payload_;
    sJwtSecret &secret_;
};

class HttpServer {
public:
    explicit HttpServer(ClusterManager *clusterManager);

    void start();

    void join();

    void stop();

    HttpServerImpl &getServer() { return this->server; }

    std::unique_ptr<sAuthorizationResult> isAuthorized(SimpleWeb::CaseInsensitiveMultimap &headers);

private:
    HttpServerImpl server;
    std::thread server_thread;
    std::vector<sJwtSecret> vJwtSecrets;

// Testing
EXPOSE_PROPERTY_FOR_TESTING(vJwtSecrets);
};

extern void JobApi(const std::string &path, HttpServer *server, ClusterManager *clusterManager);

extern void FileApi(const std::string &path, HttpServer *server, ClusterManager *clusterManager);


#endif //GWCLOUD_JOB_SERVER_HTTPSERVER_H
