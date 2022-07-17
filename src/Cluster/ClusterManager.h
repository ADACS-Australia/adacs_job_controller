//
// Created by lewis on 2/27/20.
//

#ifndef GWCLOUD_JOB_SERVER_CLUSTERMANAGER_H
#define GWCLOUD_JOB_SERVER_CLUSTERMANAGER_H

#include <thread>
#include "Cluster.h"
#include "../WebSocket/WebSocketServer.h"
#include "../Lib/GeneralUtils.h"

class ClusterManager {
public:
    ClusterManager();

    void start();
    std::shared_ptr<Cluster> handleNewConnection(WsServer::Connection* connection, const std::string& uuid);
    void removeConnection(WsServer::Connection *connection);
    std::shared_ptr<Cluster> getCluster(WsServer::Connection *connection);
    std::shared_ptr<Cluster> getCluster(const std::string& cluster);
    bool isClusterOnline(std::shared_ptr<Cluster> cluster);

private:
    [[noreturn]] void run();

    std::vector<std::shared_ptr<Cluster>> vClusters;
    std::map<WsServer::Connection*, std::shared_ptr<Cluster>> mConnectedClusters;

    void reconnectClusters();
    static void connectCluster(std::shared_ptr<Cluster> cluster, const std::string &token);

// Testing
EXPOSE_PROPERTY_FOR_TESTING(vClusters);
EXPOSE_PROPERTY_FOR_TESTING(mConnectedClusters);
EXPOSE_FUNCTION_FOR_TESTING(reconnectClusters);
};


#endif //GWCLOUD_JOB_SERVER_CLUSTERMANAGER_H
