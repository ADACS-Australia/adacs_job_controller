//
// Created by lewis on 27/07/22.
//

#ifndef GWCLOUD_JOB_SERVER_WEBSOCKETSERVERFIXTURE_H
#define GWCLOUD_JOB_SERVER_WEBSOCKETSERVERFIXTURE_H

#include "HttpServerFixture.h"
import WebSocketServer;
import IWebSocketServer;
import Application;
import IApplication;

struct WebSocketServerFixture : public HttpServerFixture {
    std::shared_ptr<IWebSocketServer> webSocketServer;
    bool bClusterManagerRunning = true;

    WebSocketServerFixture() {
        // Use the application from HttpServerFixture
        webSocketServer = application->getWebSocketServer();
        webSocketServer->start();

        BOOST_CHECK_EQUAL(acceptingConnections(8001), true);
    }

    ~WebSocketServerFixture() {
        // Finished with the servers and clients
        stopRunningWebSocket();
        webSocketServer->stop();
    }

    void stopRunningWebSocket() {
        auto concreteClusterManager = std::static_pointer_cast<ClusterManager>(clusterManager);
        concreteClusterManager->getvClusters()->front()->stop();
    }
    
    WebSocketServerFixture(WebSocketServerFixture const&) = delete;
    auto operator =(WebSocketServerFixture const&) -> WebSocketServerFixture& = delete;
    WebSocketServerFixture(WebSocketServerFixture&&) = delete;
    auto operator=(WebSocketServerFixture&&) -> WebSocketServerFixture& = delete;
};

#endif //GWCLOUD_JOB_SERVER_WEBSOCKETSERVERFIXTURE_H
