//
// Created by lewis on 2/26/20.
//

#include "HttpServer.h"

using namespace std;

HttpServer::HttpServer(ClusterManager* clusterManager) {
    server.config.port = 8000;
    server.config.address = "0.0.0.0";

    // Add the various API's
    JobApi("/job/apiv1/job/", &server, clusterManager);
    FileApi("/job/apiv1/file/", &server, clusterManager);
}

void HttpServer::start() {
    server_thread = thread([this]() {
        // Start server
        this->server.start();
    });

    cout << "API: Server listening on port " << server.config.port << endl << endl;

    server_thread.join();
}
