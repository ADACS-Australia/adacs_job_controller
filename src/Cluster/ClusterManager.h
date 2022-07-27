//
// Created by lewis on 2/27/20.
//

#ifndef GWCLOUD_JOB_SERVER_CLUSTERMANAGER_H
#define GWCLOUD_JOB_SERVER_CLUSTERMANAGER_H

#include "../Lib/GeneralUtils.h"
#include "../WebSocket/WebSocketServer.h"
#include "Cluster.h"
#include <thread>

class ClusterManager {
public:
    ClusterManager();

    void start();
    auto handleNewConnection(const std::shared_ptr<WsServer::Connection>& connection, const std::string& uuid) -> std::shared_ptr<Cluster>;
    void removeConnection(const std::shared_ptr<WsServer::Connection>& connection);
    auto getCluster(const std::shared_ptr<WsServer::Connection>& connection) -> std::shared_ptr<Cluster>;
    auto getCluster(const std::string& cluster) -> std::shared_ptr<Cluster>;
    auto isClusterOnline(const std::shared_ptr<Cluster>& cluster) -> bool;

private:
    [[noreturn]] void run();

    std::vector<std::shared_ptr<Cluster>> vClusters;
    std::map<std::shared_ptr<WsServer::Connection>, std::shared_ptr<Cluster>> mConnectedClusters;

    void reconnectClusters();
    static void connectCluster(const std::shared_ptr<Cluster>& cluster, const std::string &token);

// Testing
EXPOSE_PROPERTY_FOR_TESTING(vClusters);
EXPOSE_PROPERTY_FOR_TESTING(mConnectedClusters);
EXPOSE_FUNCTION_FOR_TESTING(reconnectClusters);
};


#endif //GWCLOUD_JOB_SERVER_CLUSTERMANAGER_H
