//
// Created by lewis on 2/27/20.
//

#ifndef GWCLOUD_JOB_SERVER_CLUSTER_H
#define GWCLOUD_JOB_SERVER_CLUSTER_H


#include <string>
#include <boost/uuid/uuid.hpp>
#include "../WebSocket/WebSocketServer.h"
#include "../Lib/Messaging/Message.h"

class ClusterManager;

class Cluster {
public:
    Cluster(std::string name, ClusterManager* pClusterManager);

    void connect(std::string token);

    std::string getName() { return name; }

    void setConnection(WsServer::Connection *pConnection);

    void queueMessage(std::vector<uint8_t>* data, Message::Priority priority);

private:
    std::string name;
    WsServer::Connection* pConnection = nullptr;
    ClusterManager* pClusterManager = nullptr;
};


#endif //GWCLOUD_JOB_SERVER_CLUSTER_H
