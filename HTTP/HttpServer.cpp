//
// Created by lewis on 2/26/20.
//

#include "HttpServer.h"

using namespace std;

HttpServer::HttpServer() {
    server.config.port = 8000;

    // Add the various API's
    JobApi("/job/", &server);
}

void HttpServer::start() {
    auto server_ptr = &server;
    server_thread = thread([&server_ptr]() {
        // Start server
        server_ptr->start();
    });

    cout << "API: Server listening on port " << server.config.port << endl << endl;

    server_thread.join();
}
