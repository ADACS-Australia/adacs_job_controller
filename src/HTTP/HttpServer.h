//
// Created by lewis on 2/26/20.
//

#ifndef GWCLOUD_JOB_SERVER_HTTPSERVER_H
#define GWCLOUD_JOB_SERVER_HTTPSERVER_H

#include <iostream>
#include <server_http.hpp>
#include <nlohmann/json.hpp>
#include "../Lib/GeneralUtils.h"

using HttpServerImpl = SimpleWeb::Server<SimpleWeb::HTTP>;

class ClusterManager;

class eNotAuthorized : public std::exception
{};

struct sJwtSecret {
    sJwtSecret(nlohmann::json jToken) {
        name = jToken["name"];
        secret = jToken["secret"];
    }

    std::string name;
    std::string secret;
};

class HttpServer {
public:
    explicit HttpServer(ClusterManager* clusterManager);
    void start();
    HttpServerImpl& getServer() { return this->server; }
    nlohmann::json isAuthorized(SimpleWeb::CaseInsensitiveMultimap& headers);
private:
    HttpServerImpl server;
    std::thread server_thread;
    std::vector<sJwtSecret> vJwtSecrets;

// Testing
EXPOSE_PROPERTY_FOR_TESTING(vJwtSecrets);
};

extern void JobApi(const std::string& path, HttpServer* server, ClusterManager* clusterManager);
extern void FileApi(const std::string& path, HttpServer* server, ClusterManager* clusterManager);


#endif //GWCLOUD_JOB_SERVER_HTTPSERVER_H
