#ifndef GWCLOUD_JOB_SERVER_WEBSOCKETCLIENTFIXTURE_H
#define GWCLOUD_JOB_SERVER_WEBSOCKETCLIENTFIXTURE_H

#include "WebSocketServerFixture.h"

struct WebSocketClientFixture : public WebSocketServerFixture {
    // NOLINTBEGIN(misc-non-private-member-variables-in-classes)
    std::shared_ptr<TestWsClient> websocketClient;
    std::thread clientThread;
    // NOLINTEND(misc-non-private-member-variables-in-classes)

    WebSocketClientFixture() {
        // Try to reconnect the clusters so that we can get a connection token to use later to connect the client
        clusterManager->callreconnectClusters();

        // Create the websocket client
        websocketClient = std::make_shared<TestWsClient>("localhost:8001/job/ws/?token=" + getLastToken());
    }

    ~WebSocketClientFixture() {
        // Finished with the client
        websocketClient->stop();
        clientThread.join();
    }

    WebSocketClientFixture(WebSocketClientFixture const&) = delete;
    auto operator =(WebSocketClientFixture const&) -> WebSocketClientFixture& = delete;
    WebSocketClientFixture(WebSocketClientFixture&&) = delete;
    auto operator=(WebSocketClientFixture&&) -> WebSocketClientFixture& = delete;

    void startWebSocketClient() {
        // Start the client
        clientThread = std::thread([&]() {
            websocketClient->start();
        });
    }
};

#endif //GWCLOUD_JOB_SERVER_WEBSOCKETCLIENTFIXTURE_H