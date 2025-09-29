#ifndef GWCLOUD_JOB_SERVER_WEBSOCKETCLIENTFIXTURE_H
#define GWCLOUD_JOB_SERVER_WEBSOCKETCLIENTFIXTURE_H

#include "WebSocketServerFixture.h"
import ClusterManager;

struct WebSocketClientFixture : public WebSocketServerFixture
{
    std::shared_ptr<TestWsClient> websocketClient;
    std::thread clientThread;

    WebSocketClientFixture()
    {
        // Try to reconnect the clusters so that we can get a connection token to use later to connect the client
        auto concreteClusterManager = std::static_pointer_cast<ClusterManager>(clusterManager);
        concreteClusterManager->callreconnectClusters();

        // Create the websocket client
        websocketClient = std::make_shared<TestWsClient>("localhost:8001/job/ws/?token=" + getLastToken());
    }

    ~WebSocketClientFixture()
    {
        // Finished with the client
        websocketClient->stop();
        if (clientThread.joinable())
        {
            clientThread.join();
        }
    }

    WebSocketClientFixture(WebSocketClientFixture const&)                    = delete;
    auto operator=(WebSocketClientFixture const&) -> WebSocketClientFixture& = delete;
    WebSocketClientFixture(WebSocketClientFixture&&)                         = delete;
    auto operator=(WebSocketClientFixture&&) -> WebSocketClientFixture&      = delete;

    void startWebSocketClient()
    {
        // Start the client
        clientThread = std::thread([&]() {
            websocketClient->start();
        });
    }
};

#endif  // GWCLOUD_JOB_SERVER_WEBSOCKETCLIENTFIXTURE_H