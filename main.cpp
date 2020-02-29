
#include "HTTP/HttpServer.h"
#include "WebSocket/WebSocketServer.h"
#include "Cluster/ClusterManager.h"

int main() {
    auto clusterManager = new ClusterManager();
    auto httpServer = new HttpServer();
    auto websocketServer = new WebSocketServer(clusterManager);

    // Start the websocket server
    websocketServer->start();

    // Now that the websocket is listening, start the cluster manager
    clusterManager->start();

    // Now finally start the http server to handle api requests
    httpServer->start();

    return 0;
}
