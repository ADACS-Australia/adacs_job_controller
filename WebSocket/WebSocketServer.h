//
// Created by lewis on 2/26/20.
//

#ifndef GWCLOUD_JOB_SERVER_WEBSOCKETSERVER_H
#define GWCLOUD_JOB_SERVER_WEBSOCKETSERVER_H

// Hack to prevent DEPRECATED from being undefined in server_ws.hpp
#ifndef DEPRECATED
#define DEPRECATED
#endif
#include "../Lib/Simple-WebSocket-Server-v2.0.0/server_ws.hpp"

using WsServer = SimpleWeb::SocketServer<SimpleWeb::WS>;
class ClusterManager;

class WebSocketServer {
public:
    WebSocketServer(ClusterManager *clusterManager);

    void start();

private:
    WsServer server;
    std::thread server_thread;

    bool accepting_connections(unsigned short port);

    ClusterManager *clusterManager;
};


#endif //GWCLOUD_JOB_SERVER_WEBSOCKETSERVER_H
