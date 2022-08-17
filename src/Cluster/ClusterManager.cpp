//
// Created by lewis on 2/27/20.
//

#include "Cluster.h"
#include "../DB/MySqlConnector.h"
#include "../Lib/jobserver_schema.h"
#include "ClusterManager.h"
#include <algorithm>
#include <boost/process.hpp>
#include <iostream>
#include <nlohmann/json.hpp>

// Define a mutex that can be used for safely removing entries from the mClusterPings map
// NOLINTNEXTLINE(cppcoreguidelines-avoid-non-const-global-variables)
std::mutex mClusterPingsDeletionLockMutex;

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
    for (const auto &jsonCluster : jsonClusters) {
        // Get the cluster details from the cluster config and create a Cluster instance
        auto cluster = std::make_shared<Cluster>(std::make_shared<sClusterDetails>(jsonCluster));
        vClusters.push_back(cluster);
    }
}

void ClusterManager::start() {
    [[maybe_unused]] static const auto clusterThread = std::make_unique<std::thread>(&ClusterManager::run, this);
    [[maybe_unused]] static const auto pingThread = std::make_unique<std::thread>(&ClusterManager::runPings, this);
}

[[noreturn]] void ClusterManager::run() {
    while (true) {
        reconnectClusters();

        // Wait CLUSTER_MANAGER_CLUSTER_RECONNECT_SECONDS to check again
        std::this_thread::sleep_for(std::chrono::seconds(CLUSTER_MANAGER_CLUSTER_RECONNECT_SECONDS));
    }
}

[[noreturn]] void ClusterManager::runPings() {
    while (true) {
        checkPings();

        // Wait CLUSTER_MANAGER_PING_INTERVAL_SECONDS to check again
        std::this_thread::sleep_for(std::chrono::seconds(CLUSTER_MANAGER_PING_INTERVAL_SECONDS));
    }
}

void ClusterManager::reconnectClusters() {
    // Create a list of threads we spawn
    std::vector<std::shared_ptr<std::thread>> vThreads;

    // Try to reconnect all cluster
    for (auto &cluster : vClusters) {
        // Check if the cluster is online
        if (!isClusterOnline(cluster)) {
            // Create a database connection
            auto database = MySqlConnector();

            // Get the tables
            schema::JobserverClusteruuid clusterUuidTable;

            // todo: Delete any existing uuids for this cluster

            // Because UUID's very occasionally collide, so try to generate a few UUID's in a row
            try {
                // Generate a UUID for this connection
                auto uuid = generateUUID();

                // Insert the UUID in the database
                database->run(
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
                // TODO(lewis): Need to better track the created thread object and dispose of it. Perhaps we need to ensure
                // TODO(lewis): that boost process in connectCluster times out after 30 seconds?
                vThreads.push_back(
                        std::make_shared<std::thread>([cluster, uuid] {
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
    for (const auto& thread : vThreads) {
        thread->join();
    }
}

auto ClusterManager::handleNewConnection(const std::shared_ptr<WsServer::Connection>& connection, const std::string &uuid) -> std::shared_ptr<Cluster> {
    // Get the tables
    schema::JobserverClusteruuid clusterUuidTable;

    // Create a database connection
    auto database = MySqlConnector();

    // First find any tokens older than CLUSTER_MANAGER_TOKEN_EXPIRY_SECONDS and delete them
    database->run(
            remove_from(clusterUuidTable)
                    .where(
                            clusterUuidTable.timestamp <=
                            std::chrono::system_clock::now() -
                            std::chrono::seconds(CLUSTER_MANAGER_TOKEN_EXPIRY_SECONDS)
                    )

    );

    // Now try to find the cluster for the connecting uuid
    auto uuidResults = database->run(
            select(all_of(clusterUuidTable))
                    .from(clusterUuidTable)
                    .where(clusterUuidTable.uuid == uuid)
    );

    // Check that the uuid was valid
    if (uuidResults.empty()) {
        return nullptr;
    }

    // Get the cluster from the uuid
    std::string sCluster = uuidResults.front().cluster;

    // Delete all records from the database for the provided cluster
    database->run(
            remove_from(clusterUuidTable)
                    .where(
                            clusterUuidTable.cluster == sCluster
                    )

    );

    // Get the cluster object from the string
    auto cluster = getCluster(sCluster);

    // Check that the cluster was valid (Should always be)
    if (!cluster) {
        return nullptr;
    }

    // Record the connected cluster and reset the ping timer
    mConnectedClusters[connection] = cluster;
    {
        std::unique_lock<std::mutex> mClusterPingsDeletionLock(mClusterPingsDeletionLockMutex);
        mClusterPings[connection] = {};
    }

    // Configure the cluster
    cluster->setConnection(connection);

    // Cluster is now connected
    return cluster;
}

void ClusterManager::removeConnection(const std::shared_ptr<WsServer::Connection>& connection, bool close, bool lock) {
    // Get the cluster for this connection
    auto pCluster = getCluster(connection);

    // Reset the cluster's connection
    if (pCluster) {
        pCluster->setConnection(nullptr);
    }

    // Remove the specified connection from the connected clusters and clear the ping timer
    mConnectedClusters.erase(connection);
    if (lock)
    {
        std::unique_lock<std::mutex> mClusterPingsDeletionLock(mClusterPingsDeletionLockMutex);
        mClusterPings.erase(connection);
    } else {
        mClusterPings.erase(connection);
    }

    // Make sure the connection is closed
    if (close) {
        connection->close();
    }

    // Try to reconnect the cluster in case it's a temporary network failure
    reconnectClusters();
}

void ClusterManager::handlePong(const std::shared_ptr<WsServer::Connection>& connection) {
    // Update the ping timer for this connection
    std::unique_lock<std::mutex> mClusterPingsDeletionLock(mClusterPingsDeletionLockMutex);

    if (mClusterPings.find(connection) != mClusterPings.end()) {
        mClusterPings[connection].pongTimestamp = std::chrono::system_clock::now();

        // Report the latency
        auto latency = mClusterPings[connection].pongTimestamp - mClusterPings[connection].pingTimestamp;
        auto cluster = getCluster(connection);

        std::cout << "WS: Cluster " << std::string(cluster ? cluster->getName() : "unknown?") << " had "
        << std::chrono::duration_cast<std::chrono::milliseconds>(latency).count() << "ms latency." << std::endl;
    }
}

void ClusterManager::checkPings() {
    std::unique_lock<std::mutex> mClusterPingsDeletionLock(mClusterPingsDeletionLockMutex);

    // Check for any websocket pings that didn't pong within CLUSTER_MANAGER_PING_INTERVAL_SECONDS, and kick the connection if so
    typeof(mClusterPings) deadConnections;
    std::copy_if(
        mClusterPings.begin(),
        mClusterPings.end(),
        std::inserter(deadConnections, deadConnections.end()),
        [](auto & item){
            std::chrono::time_point<std::chrono::system_clock> zeroTime = {};
            return (item.second.pingTimestamp != zeroTime && item.second.pongTimestamp == zeroTime);
        }
    );

    // Close and remove any dead connections
    for (auto & deadConnection : deadConnections) {
        auto cluster = getCluster(deadConnection.first);
        std::cout << "WS: Error in connection with " << std::string(cluster ? cluster->getName() : "unknown?") << ". "
                  << "Error: Websocket timed out waiting for ping." << std::endl;

        removeConnection(deadConnection.first, true, false);
    }

    // Send a fresh ping to each cluster
    for (auto & mClusterPing : mClusterPings) {
        // Update the ping timestamp
        mClusterPing.second.pingTimestamp = std::chrono::system_clock::now();
        mClusterPing.second.pongTimestamp = {};

        // Send a ping to the client
        // See https://www.rfc-editor.org/rfc/rfc6455#section-5.2 for the ping opcode 137
        mClusterPing.first->send(
            "",
            [&](const SimpleWeb::error_code &errorCode){
                // Kill the connection only if the error was not indicating success
                if (!errorCode){
                    return;
                }

                removeConnection(mClusterPing.first, true, false);
                reportWebsocketError(getCluster(mClusterPing.first), errorCode);
            },
            // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers, readability-magic-numbers)
            137
        );
    }
}

void ClusterManager::reportWebsocketError(const std::shared_ptr<Cluster>& cluster, const SimpleWeb::error_code &errorCode) {
    // Log this
    std::cout << "WS: Error in connection with " << std::string(cluster ? cluster->getName() : "unknown?") << ". "
              << "Error: " << errorCode << ", error message: " << errorCode.message() << std::endl;
}

auto ClusterManager::isClusterOnline(const std::shared_ptr<Cluster>& cluster) -> bool {
    // Check if any connected clusters matches the provided cluster
    return std::any_of(mConnectedClusters.begin(), mConnectedClusters.end(),
                       [cluster](auto other) { return other.second == cluster; });
}

auto ClusterManager::getCluster(const std::shared_ptr<WsServer::Connection>& connection) -> std::shared_ptr<Cluster> {
    // Try to find the connection
    auto result = mConnectedClusters.find(connection);

    // Return the cluster if the connection was found
    return result != mConnectedClusters.end() ? result->second : nullptr;
}

auto ClusterManager::getCluster(const std::string &cluster) -> std::shared_ptr<Cluster> {
    // Find the cluster by name
    for (auto &other : vClusters) {
        if (other->getName() == cluster) {
            return other;
        }
    }

    return nullptr;
}

void ClusterManager::connectCluster(const std::shared_ptr<Cluster>& cluster, const std::string &token) {
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