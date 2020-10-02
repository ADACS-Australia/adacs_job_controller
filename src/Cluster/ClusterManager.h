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
    Cluster* handleNewConnection(WsServer::Connection* connection, const std::string& uuid);
    void removeConnection(WsServer::Connection *connection);
    Cluster* getCluster(WsServer::Connection *connection);
    Cluster* getCluster(const std::string& cluster);
    bool isClusterOnline(Cluster *cluster);

private:
    [[noreturn]] void run();

    std::thread iClusterThread;
    std::vector<Cluster*> vClusters;
    std::map<WsServer::Connection*, Cluster*> mConnectedClusters;

    void reconnectClusters();
    static void connectCluster(Cluster *cluster, const std::string &token);

// Testing
EXPOSE_PROPERTY_FOR_TESTING(vClusters);
EXPOSE_PROPERTY_FOR_TESTING(mConnectedClusters);
EXPOSE_FUNCTION_FOR_TESTING(reconnectClusters);
};


#endif //GWCLOUD_JOB_SERVER_CLUSTERMANAGER_H
