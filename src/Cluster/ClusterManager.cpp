//
// Created by lewis on 2/27/20.
//

#include <fstream>
#include <iostream>
#include "ClusterManager.h"
#include "Cluster.h"
#include <boost/uuid/uuid_generators.hpp>
#include <boost/uuid/uuid_io.hpp>
#include <boost/lexical_cast.hpp>
#include <nlohmann/json.hpp>

using namespace nlohmann;

ClusterManager::ClusterManager() {
    // Read the cluster json config
    std::ifstream i("utils/cluster_config.json");
    json jsonClusters;
    i >> jsonClusters;

    // Create the cluster instances from the config
    for (auto jc : jsonClusters) {
        auto cluster = new Cluster(jc["name"], this);
        clusters.push_back(cluster);
    }
}

void ClusterManager::start() {
    clusterThread = std::thread(&ClusterManager::run, this);
}

void ClusterManager::run() {
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wmissing-noreturn"
    while (true) {
        reconnectClusters();

        // Wait 1 minute to check again
        std::this_thread::sleep_for(std::chrono::seconds(60));
    }
#pragma clang diagnostic pop
}

void ClusterManager::reconnectClusters() {
    // Try to reconnect all cluster
    for (auto &cluster : clusters) {
        // Check if the cluster is online
        if (!is_cluster_online(cluster)) {
            // If the cluster is not online, try to connect it
            auto uuid = boost::lexical_cast<std::string>(boost::uuids::random_generator()());
            // Record the valid uuid for this cluster
            valid_uuids[uuid] = cluster;
            // Try to connect the remote client
            cluster->connect(uuid);
        }
    }
}

Cluster *ClusterManager::handle_new_connection(WsServer::Connection *connection, const std::string &uuid) {
    // Check if the uuid is valid
    // todo: Expire tokens after 60 seconds
    if (!valid_uuids.contains(uuid))
        // Nope
        return nullptr;

    // Get the cluster for this UUID
    auto cluster = valid_uuids[uuid];

    // Remove the UUID so it can't be reused
    valid_uuids.erase(uuid);

    // Record the connected cluster
    connected_clusters[connection] = cluster;

    // Configure the cluster
    cluster->setConnection(connection);

    // Cluster is now connected
    return cluster;
}

void ClusterManager::remove_connection(WsServer::Connection* connection) {
    // Get the cluster for this connection
    auto pCluster = getCluster(connection);

    // Reset the cluster's connection
    pCluster->setConnection(nullptr);

    // Remove the specified connection from the connected clusters
    connected_clusters.erase(connection);

    // Try to reconnect the cluster in case it's a temporary network failure
    reconnectClusters();
}

bool ClusterManager::is_cluster_online(Cluster* cluster) {
    // Iterate over the connected clusters
    for (auto c : connected_clusters) {
        // Check if the cluster was found
        if (c.second == cluster)
            // Yes, the cluster is online
            return true;
    }

    // No, the cluster is not online
    return false;
}

Cluster* ClusterManager::getCluster(WsServer::Connection* connection) {
    // Try to find the connection
    auto result = connected_clusters.find(connection);

    // Return the cluster if the connection was found
    return result != connected_clusters.end() ? result->second : nullptr;
}

Cluster* ClusterManager::getCluster(const std::string& cluster) {
    // Find the cluster by name
    for (auto & i : clusters) {
        if (i->getName() == cluster)
            return i;
    }

    return nullptr;
}
