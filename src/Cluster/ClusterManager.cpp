//
// Created by lewis on 2/27/20.
//

#include <fstream>
#include <iostream>
#include "ClusterManager.h"
#include "Cluster.h"
#include "../Lib/jobserver_schema.h"
#include "../DB/MySqlConnector.h"
#include <boost/uuid/uuid_generators.hpp>
#include <boost/uuid/uuid_io.hpp>
#include <boost/lexical_cast.hpp>
#include <nlohmann/json.hpp>

using namespace nlohmann;
using namespace schema;

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
            // Create a database connection
            auto db = MySqlConnector();

            // Get the tables
            JobserverClusteruuid clusterUuidTable;

            // Because UUID's very occasionally collide, so try to generate a few UUID's in a row
            int i = 0;
            for (; i < 10; i++) {
                try {
                    // Generate a UUID for this connection
                    auto uuid = boost::lexical_cast<std::string>(boost::uuids::random_generator()());

                    // Insert the UUID in the database
                    db->run(
                            insert_into(clusterUuidTable)
                                    .set(
                                            clusterUuidTable.cluster = cluster->getName(),
                                            clusterUuidTable.uuid = uuid,
                                            clusterUuidTable.timestamp = std::chrono::system_clock::now()
                                    )
                    );

                    // The insert was successful

                    // Try to connect the remote client
                    cluster->connect(uuid);

                    // Nothing more to do
                    break;
                } catch (sqlpp::exception&) {}
            }

            // Check if the insert was successful
            if (i == 10)
                throw std::runtime_error("Unable to insert a UUID for connecting cluster " + cluster->getName());
        }
    }
}

Cluster *ClusterManager::handle_new_connection(WsServer::Connection *connection, const std::string &uuid) {
    // Get the tables
    JobserverClusteruuid clusterUuidTable;

    // Create a database connection
    auto db = MySqlConnector();

    // First find any tokens older than 60 seconds and delete them
    db->run(
            remove_from(clusterUuidTable)
                    .where(
                            clusterUuidTable.timestamp <=
                            std::chrono::system_clock::now() +
                            std::chrono::seconds(60)
                    )

    );

    // Now try to find the cluster for the connecting uuid
    auto uuidResults = db->run(
            select(all_of(clusterUuidTable))
                    .from(clusterUuidTable)
                    .where(clusterUuidTable.uuid == uuid)
    );

    // Get the cluster from the uuid
    std::string sCluster;

    // I found I had to iterate it to get the single record, as front gave me corrupted results
    for (auto &rUuid : uuidResults) {
        sCluster = std::string(rUuid.cluster);
    }

    // Check that the uuid was valid
    if (sCluster.empty())
        return nullptr;

    // Delete the uuid from the database
    db->run(
            remove_from(clusterUuidTable)
                    .where(
                            clusterUuidTable.uuid == uuid
                    )

    );

    // Get the cluster object from the string
    auto cluster = getCluster(sCluster);

    // Check that the cluster was valid (Should always be)
    if (!cluster)
        return nullptr;

    // Record the connected cluster
    connected_clusters[connection] = cluster;

    // Configure the cluster
    cluster->setConnection(connection);

    // Cluster is now connected
    return cluster;
}

void ClusterManager::remove_connection(WsServer::Connection *connection) {
    // Get the cluster for this connection
    auto pCluster = getCluster(connection);

    // Reset the cluster's connection
    pCluster->setConnection(nullptr);

    // Remove the specified connection from the connected clusters
    connected_clusters.erase(connection);

    // Try to reconnect the cluster in case it's a temporary network failure
    reconnectClusters();
}

bool ClusterManager::is_cluster_online(Cluster *cluster) {
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

Cluster *ClusterManager::getCluster(WsServer::Connection *connection) {
    // Try to find the connection
    auto result = connected_clusters.find(connection);

    // Return the cluster if the connection was found
    return result != connected_clusters.end() ? result->second : nullptr;
}

Cluster *ClusterManager::getCluster(const std::string &cluster) {
    // Find the cluster by name
    for (auto &i : clusters) {
        if (i->getName() == cluster)
            return i;
    }

    return nullptr;
}
