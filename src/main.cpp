#include "Cluster/ClusterManager.h"
#include "Interfaces/IClusterManager.h"
#include "HTTP/HttpServer.h"
#include "WebSocket/WebSocketServer.h"

auto main() -> int
{
    auto clusterManager = std::make_shared<ClusterManager>();
    auto httpServer = std::make_unique<HttpServer>(std::static_pointer_cast<IClusterManager>(clusterManager));
    auto websocketServer = std::make_unique<WebSocketServer>(clusterManager);

    // Start the websocket server
    websocketServer->start();

    // Now that the websocket is listening, start the cluster manager
    clusterManager->start();

    // Now finally start the http server to handle api requests
    httpServer->start();
    httpServer->join();

    return 0;
}
