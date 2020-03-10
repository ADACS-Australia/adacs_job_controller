//
// Created by lewis on 2/26/20.
//

#ifndef GWCLOUD_JOB_SERVER_HTTPSERVER_H
#define GWCLOUD_JOB_SERVER_HTTPSERVER_H

#include <iostream>
#include <server_http.hpp>

using HttpServerImpl = SimpleWeb::Server<SimpleWeb::HTTP>;

class ClusterManager;

extern void JobApi(const std::string& path, HttpServerImpl* server, ClusterManager* clusterManager);


class HttpServer {
public:
    explicit HttpServer(ClusterManager* clusterManager);
    void start();
private:
    HttpServerImpl server;
    std::thread server_thread;
};


#endif //GWCLOUD_JOB_SERVER_HTTPSERVER_H
