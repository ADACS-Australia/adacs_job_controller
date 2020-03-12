//
// Created by lewis on 2/26/20.
//

#include "HttpServer.h"

using namespace std;

HttpServer::HttpServer(ClusterManager* clusterManager) {
    server.config.port = 8000;

    // Add the various API's
    JobApi("/apiv1/job/", &server, clusterManager);
    FileApi("/apiv1/file/", &server, clusterManager);
}

void HttpServer::start() {
    server_thread = thread([this]() {
        // Start server
        this->server.start();
    });

    cout << "API: Server listening on port " << server.config.port << endl << endl;

    server_thread.join();
}
