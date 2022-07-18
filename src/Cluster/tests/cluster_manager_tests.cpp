//
// Created by lewis on 2/10/20.
//

#include "../ClusterManager.h"
#include "../../DB/MySqlConnector.h"
#include "../../Lib/jobserver_schema.h"
#include <boost/test/unit_test.hpp>

// NOLINTBEGIN(concurrency-mt-unsafe)

BOOST_AUTO_TEST_SUITE(ClusterManager_test_suite)
    // Define several clusters and set the environment variable
    const auto sClusters = R"(
    [
        {
            "name": "cluster1",
            "host": "cluster1.com",
            "username": "user1",
            "path": "/cluster1/",
            "key": "cluster1_key"
        },
        {
            "name": "cluster2",
            "host": "cluster2.com",
            "username": "user2",
            "path": "/cluster2/",
            "key": "cluster2_key"
        },
        {
            "name": "cluster3",
            "host": "cluster3.com",
            "username": "user3",
            "path": "/cluster3/",
            "key": "cluster3_key"
        }
    ]
    )";

    BOOST_AUTO_TEST_CASE(test_constructor) {
        // First check that instantiating ClusterManager with no cluster config works as expected
        unsetenv(CLUSTER_CONFIG_ENV_VARIABLE);
        auto mgr1 = std::make_shared<ClusterManager>();
        BOOST_CHECK_EQUAL(mgr1->getvClusters()->size(), 0);

        setenv(CLUSTER_CONFIG_ENV_VARIABLE, base64Encode(sClusters).c_str(), 1);
        auto mgr = std::make_shared<ClusterManager>();

        // Double check that the cluster json was correctly parsed
        BOOST_CHECK_EQUAL(mgr->getvClusters()->size(), 3);
        for (auto i = 1; i <= 3; i++) {
            BOOST_CHECK_EQUAL(mgr->getvClusters()->at(i-1)->getClusterDetails()->getName(), "cluster" + std::to_string(i));
            BOOST_CHECK_EQUAL(mgr->getvClusters()->at(i-1)->getClusterDetails()->getSshHost(), "cluster" + std::to_string(i) + ".com");
            BOOST_CHECK_EQUAL(mgr->getvClusters()->at(i-1)->getClusterDetails()->getSshUsername(), "user" + std::to_string(i));
            BOOST_CHECK_EQUAL(mgr->getvClusters()->at(i-1)->getClusterDetails()->getSshPath(), "/cluster" + std::to_string(i) + "/");
            BOOST_CHECK_EQUAL(mgr->getvClusters()->at(i-1)->getClusterDetails()->getSshKey(), "cluster" + std::to_string(i) + "_key");
        }

        // There should be no connected clusters
        BOOST_CHECK_EQUAL(mgr->getmConnectedClusters()->size(), 0);
    }

    BOOST_AUTO_TEST_CASE(test_getCluster) {
        setenv(CLUSTER_CONFIG_ENV_VARIABLE, base64Encode(sClusters).c_str(), 1);
        auto mgr = std::make_shared<ClusterManager>();

        // First test getting clusters by name
        for (const auto& cluster : *mgr->getvClusters()) {
            BOOST_CHECK_EQUAL(mgr->getCluster(cluster->getClusterDetails()->getName()), cluster);
        }

        // Check that getting a cluster by invalid name returns null
        BOOST_CHECK_EQUAL(mgr->getCluster("not_a_real_cluster"), nullptr);

        // Add connected clusters
        std::map<std::shared_ptr<WsServer::Connection>, std::shared_ptr<Cluster>> connections;
        for (auto cluster : *mgr->getvClusters()) {
            auto con = std::make_shared<WsServer::Connection>(nullptr);
            mgr->getmConnectedClusters()->emplace(con, cluster);
            connections[con] = cluster;
        }

        // Check that getting cluster by connection works correctly
        for (const auto& con : connections) {
            BOOST_CHECK_EQUAL(mgr->getCluster(con.first), con.second);
        }

        // Check that getting a cluster by an invalid connection returns null
        auto ptr = std::make_shared<WsServer::Connection>(nullptr);
        BOOST_CHECK_EQUAL(mgr->getCluster(ptr), nullptr);
    }

    BOOST_AUTO_TEST_CASE(test_isClusterOnline) {
        setenv(CLUSTER_CONFIG_ENV_VARIABLE, base64Encode(sClusters).c_str(), 1);
        auto mgr = std::make_shared<ClusterManager>();

        // Add a connected cluster
        auto con = std::make_shared<WsServer::Connection>(nullptr);
        mgr->getmConnectedClusters()->emplace(con, mgr->getvClusters()->at(1));

        // Check that the first and third clusters are offline
        BOOST_CHECK_EQUAL(mgr->isClusterOnline(mgr->getvClusters()->at(0)), false);
        BOOST_CHECK_EQUAL(mgr->isClusterOnline(mgr->getvClusters()->at(2)), false);

        // Check that the second cluster is online
        BOOST_CHECK_EQUAL(mgr->isClusterOnline(mgr->getvClusters()->at(1)), true);
    }

    BOOST_AUTO_TEST_CASE(test_removeConnection) {
        setenv(CLUSTER_CONFIG_ENV_VARIABLE, base64Encode(sClusters).c_str(), 1);
        auto mgr = std::make_shared<ClusterManager>();

        // Add connected clusters
        std::map<std::shared_ptr<WsServer::Connection>, std::shared_ptr<Cluster>> connections;
        for (auto cluster : *mgr->getvClusters()) {
            auto con = std::make_shared<WsServer::Connection>(nullptr);
            mgr->getmConnectedClusters()->emplace(con, cluster);
            connections[con] = cluster;
            cluster->setpConnection(con);
        }

        // Check that cluster is really connected
        BOOST_CHECK_EQUAL(mgr->getCluster(*mgr->getvClusters()->at(1)->getpConnection()), mgr->getvClusters()->at(1));

        // Remember connection for second cluster
        auto con = *mgr->getvClusters()->at(1)->getpConnection();

        // Remove the second cluster connection
        mgr->removeConnection(con);

        // Check removing an invalid connection
        {
            auto ptr = std::make_shared<WsServer::Connection>(nullptr);
            mgr->removeConnection(ptr);
        }
        mgr->removeConnection(nullptr);

        // Check that the connection has been removed correctly
        // Connection in second cluster should now be null
        BOOST_CHECK_EQUAL(*mgr->getvClusters()->at(1)->getpConnection(), nullptr);

        // Check that the connection no longer exists in the connected clusters
        BOOST_CHECK_MESSAGE(mgr->getmConnectedClusters()->find(con) == mgr->getmConnectedClusters()->end(), "mgr->getmConnectedClusters().find(con) == mgr->getmConnectedClusters().end()");

        // Check that the cluster is no longer connected
        BOOST_CHECK_EQUAL(mgr->getCluster(*mgr->getvClusters()->at(1)->getpConnection()), nullptr);

        // Check that remaining clusters are still connected
        BOOST_CHECK_EQUAL(mgr->getCluster(*mgr->getvClusters()->at(0)->getpConnection()), mgr->getvClusters()->at(0));
        BOOST_CHECK_EQUAL(mgr->getCluster(*mgr->getvClusters()->at(2)->getpConnection()), mgr->getvClusters()->at(2));
    }

    BOOST_AUTO_TEST_CASE(test_reconnectClusters) {
        // First make sure we delete all entries from the uuid table
        auto database = MySqlConnector();
        schema::JobserverClusteruuid clusterUuidTable;
        database->run(remove_from(clusterUuidTable).unconditionally());

        setenv(CLUSTER_CONFIG_ENV_VARIABLE, base64Encode(sClusters).c_str(), 1);
        auto mgr = std::make_shared<ClusterManager>();

        // Check that reconnectClusters tries to reconnect all clusters initially
        mgr->callreconnectClusters();

        // There should be 3 uuid records in the database
        auto uuidResults = database->run(select(all_of(clusterUuidTable)).from(clusterUuidTable).unconditionally());
        auto uuidResultsCount = 0;
        for (const auto &rUuid : uuidResults) {
            uuidResultsCount++;
        }
        BOOST_CHECK_EQUAL(uuidResultsCount, 3);

        // Mark a cluster as connected, and try again, there should be only 2 uuids
        database->run(remove_from(clusterUuidTable).unconditionally());

        auto con = std::make_shared<WsServer::Connection>(nullptr);
        mgr->getmConnectedClusters()->emplace(con, mgr->getvClusters()->at(1));

        mgr->callreconnectClusters();

        uuidResults = database->run(select(all_of(clusterUuidTable)).from(clusterUuidTable).unconditionally());
        uuidResultsCount = 0;
        for (const auto &rUuid : uuidResults) {
            uuidResultsCount++;
        }
        BOOST_CHECK_EQUAL(uuidResultsCount, 2);
    }

    BOOST_AUTO_TEST_CASE(test_handleNewConnection) {
        // First make sure we delete all entries from the uuid table
        auto database = MySqlConnector();
        schema::JobserverClusteruuid clusterUuidTable;
        database->run(remove_from(clusterUuidTable).unconditionally());

        setenv(CLUSTER_CONFIG_ENV_VARIABLE, base64Encode(sClusters).c_str(), 1);
        auto mgr = std::make_shared<ClusterManager>();

        // Make sure that old uuids are deleted
        database->run(
                insert_into(clusterUuidTable)
                        .set(
                                clusterUuidTable.cluster = mgr->getvClusters()->at(0)->getClusterDetails()->getName(),
                                clusterUuidTable.uuid = "uuid_doesn't_matter_here",
                                clusterUuidTable.timestamp = std::chrono::system_clock::now() - std::chrono::seconds(CLUSTER_MANAGER_TOKEN_EXPIRY_SECONDS)
                        )
        );

        {
            auto ptr = std::make_shared<WsServer::Connection>(nullptr);
            mgr->handleNewConnection(ptr, "not_a_real_uuid");
        }

        // There should be 0 uuid records in the database
        auto uuidResults = database->run(select(all_of(clusterUuidTable)).from(clusterUuidTable).unconditionally());
        auto uuidResultsCount = 0;
        for (const auto &rUuid : uuidResults) {
            uuidResultsCount++;
        }
        BOOST_CHECK_EQUAL(uuidResultsCount, 0);

        // Make sure that old uuids are deleted
        database->run(
                insert_into(clusterUuidTable)
                        .set(
                                clusterUuidTable.cluster = mgr->getvClusters()->at(0)->getClusterDetails()->getName(),
                                clusterUuidTable.uuid = "uuid_doesn't_matter_here",
                                clusterUuidTable.timestamp = std::chrono::system_clock::now() - std::chrono::seconds(CLUSTER_MANAGER_TOKEN_EXPIRY_SECONDS-1)
                        )
        );

        {
            auto ptr = std::make_shared<WsServer::Connection>(nullptr);
            mgr->handleNewConnection(ptr, "not_a_real_uuid");
        }

        // There should be 1 uuid records in the database
        uuidResults = database->run(select(all_of(clusterUuidTable)).from(clusterUuidTable).unconditionally());
        uuidResultsCount = 0;
        for (const auto &rUuid : uuidResults) {
            uuidResultsCount++;
        }
        BOOST_CHECK_EQUAL(uuidResultsCount, 1);

        // Delete all uuid records again
        database->run(remove_from(clusterUuidTable).unconditionally());

        // Insert 5 uuids for a fake cluster
        std::string last_uuid;
        for (auto i = 0; i < 5; i++) { // NOLINT(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
            last_uuid = generateUUID();
            database->run(
                    insert_into(clusterUuidTable)
                            .set(
                                    clusterUuidTable.cluster = "not_real_cluster",
                                    clusterUuidTable.uuid = last_uuid,
                                    clusterUuidTable.timestamp = std::chrono::system_clock::now()
                            )
            );
        }

        // Make sure all clusters are currently unconnected
        for (const auto& cluster : *mgr->getvClusters()) {
            BOOST_CHECK_EQUAL(mgr->isClusterOnline(cluster), false);
        }

        auto con = std::make_shared<WsServer::Connection>(nullptr);
        mgr->handleNewConnection(con, last_uuid);

        // All uuids should be deleted (because uuid was in database), and no clusters connected
        uuidResults = database->run(select(all_of(clusterUuidTable)).from(clusterUuidTable).unconditionally());
        uuidResultsCount = 0;
        for (const auto &rUuid : uuidResults) {
            uuidResultsCount++;
        }
        BOOST_CHECK_EQUAL(uuidResultsCount, 0);

        for (const auto& cluster : *mgr->getvClusters()) {
            BOOST_CHECK_EQUAL(mgr->isClusterOnline(cluster), false);
        }

        // Insert 5 uuids for a real cluster
        for (auto i = 0; i < 5; i++) { // NOLINT(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
            last_uuid = generateUUID();
            database->run(
                    insert_into(clusterUuidTable)
                            .set(
                                    clusterUuidTable.cluster = mgr->getvClusters()->at(1)->getName(),
                                    clusterUuidTable.uuid = last_uuid,
                                    clusterUuidTable.timestamp = std::chrono::system_clock::now()
                            )
            );
        }

        // Make sure all clusters are currently unconnected
        for (const auto& cluster : *mgr->getvClusters()) {
            BOOST_CHECK_EQUAL(mgr->isClusterOnline(cluster), false);
        }

        {
            auto con = std::make_shared<WsServer::Connection>(nullptr);
            mgr->handleNewConnection(con, last_uuid);

            // There should be no uuids left in the database
            uuidResults = database->run(select(all_of(clusterUuidTable)).from(clusterUuidTable).unconditionally());
            uuidResultsCount = 0;
            for (const auto &rUuid: uuidResults) {
                uuidResultsCount++;
            }
            BOOST_CHECK_EQUAL(uuidResultsCount, 0);

            // The second cluster should now be connected
            BOOST_CHECK_EQUAL(mgr->isClusterOnline(mgr->getvClusters()->at(1)), true);
        }
    }

BOOST_AUTO_TEST_SUITE_END()

// NOLINTEND(concurrency-mt-unsafe)