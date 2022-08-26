//
// Created by lewis on 2/10/20.
//

#include "../ClusterManager.h"
#include "../../DB/MySqlConnector.h"
#include "../../Lib/jobserver_schema.h"
#include "../../tests/fixtures/DatabaseFixture.h"
#include "../../tests/fixtures/HttpServerFixture.h"
#include <boost/test/unit_test.hpp>

// NOLINTBEGIN(concurrency-mt-unsafe)

struct ClusterManagerTestDataFixture : public DatabaseFixture, public HttpServerFixture {
    // NOLINTBEGIN(misc-non-private-member-variables-in-classes)
    std::shared_ptr<ClusterManager> mgr;
    // NOLINTEND(misc-non-private-member-variables-in-classes)

    ClusterManagerTestDataFixture() {
        mgr = std::make_shared<ClusterManager>();
    }
};

BOOST_FIXTURE_TEST_SUITE(ClusterManager_test_suite, ClusterManagerTestDataFixture)
    BOOST_AUTO_TEST_CASE(test_constructor) {
        // First check that instantiating ClusterManager with no cluster config works as expected
        unsetenv(CLUSTER_CONFIG_ENV_VARIABLE);
        auto mgr = std::make_shared<ClusterManager>();
        BOOST_CHECK_EQUAL(mgr->getvClusters()->size(), 0);

        setenv(CLUSTER_CONFIG_ENV_VARIABLE, base64Encode(sClusters).c_str(), 1);
        mgr = std::make_shared<ClusterManager>();

        // Double check that the cluster json was correctly parsed
        BOOST_CHECK_EQUAL(mgr->getvClusters()->size(), 3);
        for (auto i = 1; i <= mgr->getvClusters()->size(); i++) {
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
        // First test getting clusters by name
        for (const auto& cluster : *mgr->getvClusters()) {
            BOOST_CHECK_EQUAL(mgr->getCluster(cluster->getClusterDetails()->getName()), cluster);
        }

        // Check that getting a cluster by invalid name returns null
        BOOST_CHECK_EQUAL(mgr->getCluster("not_a_real_cluster"), nullptr);

        // Add connected clusters
        std::map<std::shared_ptr<WsServer::Connection>, std::shared_ptr<Cluster>> connections;
        for (const auto& cluster : *mgr->getvClusters()) {
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
        // Add connected clusters
        std::map<std::shared_ptr<WsServer::Connection>, std::shared_ptr<Cluster>> connections;
        for (const auto& cluster : *mgr->getvClusters()) {
            auto con = std::make_shared<WsServer::Connection>(nullptr);
            mgr->getmConnectedClusters()->emplace(con, cluster);
            mgr->getmClusterPings()->emplace(con, ClusterManager::sPingPongTimes{});
            connections[con] = cluster;
            cluster->setpConnection(con);
        }

        // Check that cluster is really connected and that the cluster ping map has been correctly set up
        BOOST_CHECK_EQUAL(mgr->getCluster(*mgr->getvClusters()->at(1)->getpConnection()), mgr->getvClusters()->at(1));

        // Remember connection for second cluster
        auto con = *mgr->getvClusters()->at(1)->getpConnection();

        // Remove the second cluster connection
        mgr->removeConnection(con, false);

        // Check removing an invalid connection
        {
            auto ptr = std::make_shared<WsServer::Connection>(nullptr);
            mgr->removeConnection(ptr, false);
        }
        mgr->removeConnection(nullptr, false);

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

        // Check that the mClusterPing map is correct
        BOOST_CHECK_EQUAL(mgr->getmClusterPings()->find(*mgr->getvClusters()->at(0)->getpConnection())->first, *mgr->getvClusters()->at(0)->getpConnection());
        BOOST_CHECK_MESSAGE(mgr->getmClusterPings()->find(*mgr->getvClusters()->at(1)->getpConnection()) == mgr->getmClusterPings()->end(), "mClusterPings was not consistent");
        BOOST_CHECK_EQUAL(mgr->getmClusterPings()->find(*mgr->getvClusters()->at(2)->getpConnection())->first, *mgr->getvClusters()->at(2)->getpConnection());
    }

    BOOST_AUTO_TEST_CASE(test_reconnectClusters) {
        // Check that reconnectClusters tries to reconnect all clusters initially
        mgr->callreconnectClusters();

        // There should be 3 uuid records in the database
        uint64_t uuidResultsCount = database->run(select(sqlpp::count(1)).from(jobClusteruuid).unconditionally()).front().count;
        BOOST_CHECK_EQUAL(uuidResultsCount, 3);

        // Mark a cluster as connected, and try again, there should be only 2 uuids
        database->run(remove_from(jobClusteruuid).unconditionally());

        auto con = std::make_shared<WsServer::Connection>(nullptr);
        mgr->getmConnectedClusters()->emplace(con, mgr->getvClusters()->at(1));

        mgr->callreconnectClusters();

        uuidResultsCount = database->run(select(sqlpp::count(1)).from(jobClusteruuid).unconditionally()).front().count;
        BOOST_CHECK_EQUAL(uuidResultsCount, 2);
    }

    BOOST_AUTO_TEST_CASE(test_handleNewConnection_expire_uuids) {
        // Make sure that old expired uuids are deleted from the database when a cluster is connecting. Note that
        // we're taking advantage of the UUID field being unique here. So we expect that the UUID
        // "uuid_doesn't_matter_here" will be removed by mgr->handleNewConnection because the UUID is entered in to the
        // database already expired. When we try to insert the same UUID again afterwards, the database will raise an
        // exception if the functionality does not work as expected. There is also an additional separate check to
        // confirm that the number of UUID's in the database is 0 after the handleNewConnection call.
        database->run(
                insert_into(jobClusteruuid)
                        .set(
                                jobClusteruuid.cluster = mgr->getvClusters()->at(0)->getClusterDetails()->getName(),
                                jobClusteruuid.uuid = "uuid_doesn't_matter_here",
                                jobClusteruuid.timestamp = std::chrono::system_clock::now() -
                                                             std::chrono::seconds(CLUSTER_MANAGER_TOKEN_EXPIRY_SECONDS)
                        )
        );

        {
            auto ptr = std::make_shared<WsServer::Connection>(nullptr);
            mgr->handleNewConnection(ptr, "not_a_real_uuid");
        }

        // There should be 0 uuid records in the database because the previous uuid was expired
        uint64_t uuidResultsCount = database->run(select(sqlpp::count(1)).from(jobClusteruuid).unconditionally()).front().count;
        BOOST_CHECK_EQUAL(uuidResultsCount, 0);

        // Make sure that old expired uuids are deleted
        database->run(
                insert_into(jobClusteruuid)
                        .set(
                                jobClusteruuid.cluster = mgr->getvClusters()->at(0)->getClusterDetails()->getName(),
                                jobClusteruuid.uuid = "uuid_doesn't_matter_here",
                                jobClusteruuid.timestamp = std::chrono::system_clock::now() - std::chrono::seconds(
                                        CLUSTER_MANAGER_TOKEN_EXPIRY_SECONDS - 1)
                        )
        );

        {
            auto ptr = std::make_shared<WsServer::Connection>(nullptr);
            mgr->handleNewConnection(ptr, "not_a_real_uuid");
        }

        // There should be 1 uuid records in the database because the previous uuid is not yet expired
        uuidResultsCount = database->run(select(sqlpp::count(1)).from(jobClusteruuid).unconditionally()).front().count;
        BOOST_CHECK_EQUAL(uuidResultsCount, 1);
    }

    BOOST_AUTO_TEST_CASE(test_handleNewConnection_valid_uuid) {
        // Insert 5 uuids for a fake cluster
        std::string last_uuid;
        for (auto i = 0; i < 5; i++) { // NOLINT(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
            last_uuid = generateUUID();
            database->run(
                    insert_into(jobClusteruuid)
                            .set(
                                    jobClusteruuid.cluster = "not_real_cluster",
                                    jobClusteruuid.uuid = last_uuid,
                                    jobClusteruuid.timestamp = std::chrono::system_clock::now()
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
        uint64_t uuidResultsCount = database->run(select(sqlpp::count(1)).from(jobClusteruuid).unconditionally()).front().count;
        BOOST_CHECK_EQUAL(uuidResultsCount, 0);

        for (const auto& cluster : *mgr->getvClusters()) {
            BOOST_CHECK_EQUAL(mgr->isClusterOnline(cluster), false);
        }

        // Insert 5 uuids for a real cluster
        for (auto i = 0; i < 5; i++) { // NOLINT(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
            last_uuid = generateUUID();
            database->run(
                    insert_into(jobClusteruuid)
                            .set(
                                    jobClusteruuid.cluster = mgr->getvClusters()->at(1)->getName(),
                                    jobClusteruuid.uuid = last_uuid,
                                    jobClusteruuid.timestamp = std::chrono::system_clock::now()
                            )
            );
        }

        // Make sure all clusters are currently unconnected
        for (const auto& cluster : *mgr->getvClusters()) {
            BOOST_CHECK_EQUAL(mgr->isClusterOnline(cluster), false);
        }

        auto clusterConnection = std::make_shared<WsServer::Connection>(nullptr);
        BOOST_ASSERT_MSG(mgr->handleNewConnection(clusterConnection, last_uuid) != nullptr, "handleNewConnection return nullptr which it should not have");

        // There should be no uuids left in the database
        uuidResultsCount = database->run(select(sqlpp::count(1)).from(jobClusteruuid).unconditionally()).front().count;
        BOOST_CHECK_EQUAL(uuidResultsCount, 0);

        // The second cluster should now be connected
        BOOST_CHECK_EQUAL(mgr->isClusterOnline(mgr->getvClusters()->at(1)), true);

        BOOST_CHECK_EQUAL(mgr->getmClusterPings()->begin()->first, *mgr->getvClusters()->at(1)->getpConnection());
        std::chrono::time_point<std::chrono::system_clock> zeroTime = {};
        BOOST_CHECK_MESSAGE(mgr->getmClusterPings()->begin()->second.pingTimestamp == zeroTime, "pingTimestamp was not zero when it should have been");
        BOOST_CHECK_MESSAGE(mgr->getmClusterPings()->begin()->second.pongTimestamp == zeroTime, "pongTimestamp was not zero when it should have been");


        // Next check that a cluster can not be connected again if it's already connected
        // Insert another UUID for the same cluster
        last_uuid = generateUUID();
        database->run(
                insert_into(jobClusteruuid)
                        .set(
                                jobClusteruuid.cluster = mgr->getvClusters()->at(1)->getName(),
                                jobClusteruuid.uuid = last_uuid,
                                jobClusteruuid.timestamp = std::chrono::system_clock::now()
                        )
        );

        con = std::make_shared<WsServer::Connection>(nullptr);
        BOOST_ASSERT_MSG(mgr->handleNewConnection(con, last_uuid) == nullptr, "handleNewConnection did not return nullptr which it should have");

        // There should be no uuids left in the database
        uuidResultsCount = database->run(select(sqlpp::count(1)).from(jobClusteruuid).unconditionally()).front().count;
        BOOST_CHECK_EQUAL(uuidResultsCount, 0);

        // The connection for the cluster should still be the originally set connection
        BOOST_CHECK_EQUAL(*mgr->getvClusters()->at(1)->getpConnection(), clusterConnection);

        // No cluster ping entry should exist for this connection
        BOOST_ASSERT_MSG(mgr->getmClusterPings()->find(con) == mgr->getmClusterPings()->end(), "mClusterPings was updated with the invalid connection when it should not have been");
    }

BOOST_AUTO_TEST_SUITE_END()

// NOLINTEND(concurrency-mt-unsafe)