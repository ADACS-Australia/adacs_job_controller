//
// Created by lewis on 2/27/20.
//

#include <fstream>
#include <iostream>
#include "ClusterManager.h"
#include "Cluster.h"
#include "../Lib/jobserver_schema.h"
#include "../DB/MySqlConnector.h"
#include <nlohmann/json.hpp>
#include <boost/process.hpp>

using namespace nlohmann;
using namespace schema;

ClusterManager::ClusterManager() {
    // Read the cluster configuration from the environment
    auto jsonClusters = nlohmann::json::parse(
            base64Decode(
                    GET_ENV(
                            CLUSTER_CONFIG_ENV_VARIABLE,
                            base64Encode("{}")
                    )
            )
    );

    // Create the cluster instances from the config
    for (const auto &jc : jsonClusters) {
        // Get the cluster details from the cluster config and create a Cluster instance
        auto cluster = std::make_shared<Cluster>(std::make_shared<sClusterDetails>(jc));
        vClusters.push_back(cluster);
    }
}

void ClusterManager::start() {
    std::thread(&ClusterManager::run, this);
}

[[noreturn]] void ClusterManager::run() {
    while (true) {
        reconnectClusters();

        // Wait 1 minute to check again
        std::this_thread::sleep_for(std::chrono::seconds(60));
    }
}

void ClusterManager::reconnectClusters() {
    // Create a list of threads we spawn
    std::vector<std::thread *> vThreads;

    // Try to reconnect all cluster
    for (auto &cluster : vClusters) {
        // Check if the cluster is online
        if (!isClusterOnline(cluster)) {
            // Create a database connection
            auto db = MySqlConnector();

            // Get the tables
            JobserverClusteruuid clusterUuidTable;

            // todo: Delete any existing uuids for this cluster

            // Because UUID's very occasionally collide, so try to generate a few UUID's in a row
            try {
                // Generate a UUID for this connection
                auto uuid = generateUUID();

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
                // Here we provide parameters by copy rather than reference
                // TODO: Need to better track the created thread object and dispose of it. Perhaps we need to ensure
                // TODO: that boost process in connectCluster times out after 30 seconds?
                vThreads.push_back(
                        new std::thread([cluster, uuid] {
                            connectCluster(cluster, uuid);
                        })
                );
            } catch (std::exception& e) {
                dumpExceptions(e);
                // Should only happen if an *extremely* rare UUID collision happens
            }
        }
    }

    // Wait for all connection threads to finish
    for (auto t : vThreads) {
        t->join();
        delete t;
    }
}

std::shared_ptr<Cluster> ClusterManager::handleNewConnection(WsServer::Connection *connection, const std::string &uuid) {
    // Get the tables
    JobserverClusteruuid clusterUuidTable;

    // Create a database connection
    auto db = MySqlConnector();

    // First find any tokens older than 60 seconds and delete them
    db->run(
            remove_from(clusterUuidTable)
                    .where(
                            clusterUuidTable.timestamp <=
                            std::chrono::system_clock::now() -
                            std::chrono::seconds(60)
                    )

    );

    // Now try to find the cluster for the connecting uuid
    auto uuidResults = db->run(
            select(all_of(clusterUuidTable))
                    .from(clusterUuidTable)
                    .where(clusterUuidTable.uuid == uuid)
    );

    // Check that the uuid was valid
    if (uuidResults.empty())
        return nullptr;

    // Get the cluster from the uuid
    std::string sCluster = uuidResults.front().cluster;

    // Delete all records from the database for the provided cluster
    db->run(
            remove_from(clusterUuidTable)
                    .where(
                            clusterUuidTable.cluster == sCluster
                    )

    );

    // Get the cluster object from the string
    auto cluster = getCluster(sCluster);

    // Check that the cluster was valid (Should always be)
    if (!cluster)
        return nullptr;

    // Record the connected cluster
    mConnectedClusters[connection] = cluster;

    // Configure the cluster
    cluster->setConnection(connection);

    // Cluster is now connected
    return cluster;
}

void ClusterManager::removeConnection(WsServer::Connection *connection) {
    // Get the cluster for this connection
    auto pCluster = getCluster(connection);

    // Reset the cluster's connection
    if (pCluster)
        pCluster->setConnection(nullptr);

    // Remove the specified connection from the connected clusters
    mConnectedClusters.erase(connection);

    // Try to reconnect the cluster in case it's a temporary network failure
    reconnectClusters();
}

bool ClusterManager::isClusterOnline(std::shared_ptr<Cluster> cluster) {
    // Check if any connected clusters matches the provided cluster
    return std::any_of(mConnectedClusters.begin(), mConnectedClusters.end(),
                       [cluster](auto c) { return c.second == cluster; });
}

std::shared_ptr<Cluster> ClusterManager::getCluster(WsServer::Connection *connection) {
    // Try to find the connection
    auto result = mConnectedClusters.find(connection);

    // Return the cluster if the connection was found
    return result != mConnectedClusters.end() ? result->second : nullptr;
}

std::shared_ptr<Cluster> ClusterManager::getCluster(const std::string &cluster) {
    // Find the cluster by name
    for (auto &i : vClusters) {
        if (i->getName() == cluster)
            return i;
    }

    return nullptr;
}

void ClusterManager::connectCluster(std::shared_ptr<Cluster> cluster, const std::string &token) {
    std::cout << "Attempting to connect cluster " << cluster->getName() << " with token " << token << std::endl;
#ifndef BUILD_TESTS
    boost::process::system(
            "./utils/keyserver/venv/bin/python ./utils/keyserver/keyserver.py",
            boost::process::env["SSH_HOST"] = cluster->getClusterDetails()->getSshHost(),
            boost::process::env["SSH_USERNAME"] = cluster->getClusterDetails()->getSshUsername(),
            boost::process::env["SSH_KEY"] = cluster->getClusterDetails()->getSshKey(),
            boost::process::env["SSH_PATH"] = cluster->getClusterDetails()->getSshPath(),
            boost::process::env["SSH_TOKEN"] = token
    );
#endif
}
