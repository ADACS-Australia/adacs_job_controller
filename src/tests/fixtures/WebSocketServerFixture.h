//
// Created by lewis on 27/07/22.
//

#ifndef GWCLOUD_JOB_SERVER_WEBSOCKETSERVERFIXTURE_H
#define GWCLOUD_JOB_SERVER_WEBSOCKETSERVERFIXTURE_H

#include "HttpServerFixture.h"

struct WebSocketServerFixture : public HttpServerFixture {
    // NOLINTBEGIN(misc-non-private-member-variables-in-classes)
    std::shared_ptr<WebSocketServer> webSocketServer;
    bool bClusterManagerRunning = true;
    // NOLINTEND(misc-non-private-member-variables-in-classes)

    WebSocketServerFixture() {
        // Set up the test websocket server
        webSocketServer = std::make_shared<WebSocketServer>(clusterManager);
        webSocketServer->start();

        BOOST_CHECK_EQUAL(acceptingConnections(8001), true);
    }

    ~WebSocketServerFixture() {
        // Finished with the servers and clients
        stopRunningWebSocket();
        webSocketServer->stop();
    }

    void stopRunningWebSocket() {
        clusterManager->getvClusters()->front()->stop();
    }
    
    WebSocketServerFixture(WebSocketServerFixture const&) = delete;
    auto operator =(WebSocketServerFixture const&) -> WebSocketServerFixture& = delete;
    WebSocketServerFixture(WebSocketServerFixture&&) = delete;
    auto operator=(WebSocketServerFixture&&) -> WebSocketServerFixture& = delete;
};

#endif //GWCLOUD_JOB_SERVER_WEBSOCKETSERVERFIXTURE_H
