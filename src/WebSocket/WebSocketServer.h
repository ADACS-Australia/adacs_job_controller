//
// Created by lewis on 2/26/20.
//

#ifndef GWCLOUD_JOB_SERVER_WEBSOCKETSERVER_H
#define GWCLOUD_JOB_SERVER_WEBSOCKETSERVER_H

// Hack to prevent DEPRECATED from being undefined in server_ws.hpp
#ifndef DEPRECATED
#define DEPRECATED
#endif
#include <server_ws.hpp>

using WsServer = SimpleWeb::SocketServer<SimpleWeb::WS>;
class ClusterManager;

class WebSocketServer {
public:
    explicit WebSocketServer(std::shared_ptr<ClusterManager>  clusterManager);

    void start();
    void join();
    void stop();

private:
    WsServer server;
    std::thread server_thread;

    std::shared_ptr<ClusterManager> clusterManager;
};


#endif //GWCLOUD_JOB_SERVER_WEBSOCKETSERVER_H
