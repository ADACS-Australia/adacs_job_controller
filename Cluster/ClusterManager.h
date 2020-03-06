//
// Created by lewis on 2/27/20.
//

#ifndef GWCLOUD_JOB_SERVER_CLUSTERMANAGER_H
#define GWCLOUD_JOB_SERVER_CLUSTERMANAGER_H

#include <thread>
#include "../Lib/json.hpp"
#include "Cluster.h"
#include "../WebSocket/WebSocketServer.h"


class ClusterManager {
public:
    ClusterManager();
    void start();
    Cluster* handle_new_connection(WsServer::Connection* connection, const std::string& uuid);
    void remove_connection(WsServer::Connection *connection);
    Cluster *getCluster(WsServer::Connection *connection);
    Cluster* getCluster(const std::string& cluster);

private:
    void run();

    std::thread clusterThread;
    std::vector<Cluster*> clusters;
    std::map<std::string, Cluster*> valid_uuids;
    std::map<WsServer::Connection*, Cluster*> connected_clusters;

    bool is_cluster_online(Cluster *cluster);
};


#endif //GWCLOUD_JOB_SERVER_CLUSTERMANAGER_H
