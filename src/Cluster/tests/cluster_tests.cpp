//
// Created by lewis on 2/10/20.
//

#include <boost/test/unit_test.hpp>
#include <random>
#include <client_ws.hpp>
#include <server_http.hpp>
#include <server_https.hpp>
#include <boost/lexical_cast.hpp>
#include <boost/uuid/uuid_io.hpp>
#include <boost/uuid/random_generator.hpp>
#include <fstream>
#include "../Cluster.h"
#include "../../Settings.h"
#include "../ClusterManager.h"
#include "../../Lib/jobserver_schema.h"
#include "../../DB/MySqlConnector.h"
#include "../../Lib/JobStatus.h"
#include "test.key.h"
#include "test.crt.h"

std::default_random_engine rng;
bool bSeeded = false;

uint32_t randomInt(uint32_t start, uint32_t end) {
    if (!bSeeded) {
        bSeeded = true;
        rng.seed(std::chrono::system_clock::now().time_since_epoch().count());
    }

    std::uniform_int_distribution<uint32_t> rng_dist(start, end);
    return rng_dist(rng);
}

std::vector<uint8_t> *generateRandomData(uint32_t count) {
    auto result = new std::vector<uint8_t>();
    result->reserve(count);

    for (uint32_t i = 0; i < count; i++) {
        result->push_back(randomInt(0, 255));
    }

    return result;
}

void writeCertFiles() {
    std::ofstream crt;
    crt.open("test.crt", std::ios::out);
    crt.write((char *) test_crt, test_crt_len);
    crt.close();

    std::ofstream key;
    key.open("test.key", std::ios::out);
    key.write((char *) test_key, test_key_len);
    key.close();
}

typedef SimpleWeb::SocketServer<SimpleWeb::WS> WsServer;
typedef SimpleWeb::SocketClient<SimpleWeb::WS> WsClient;
typedef SimpleWeb::Server<SimpleWeb::HTTP> HttpServer;
typedef SimpleWeb::Server<SimpleWeb::HTTPS> HttpsServer;

BOOST_AUTO_TEST_SUITE(Cluster_test_suite)
    // Define several clusters to use for setting the cluster config environment variable
    auto sClusters = R"(
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
        // Parse the cluster configuration
        auto jsonClusters = nlohmann::json::parse(sClusters);

        // Get the details for the first cluster
        auto details = new sClusterDetails(jsonClusters[0]);

        // Create a new cluster manager
        auto manager = new ClusterManager();

        // Check that the cluster constructor works as expected
        auto cluster = new Cluster(details, manager);

        // Check that the cluster details and the manager are set correctly
        BOOST_CHECK_EQUAL(*cluster->getpClusterDetails(), details);
        BOOST_CHECK_EQUAL(*cluster->getpClusterManager(), manager);

        // Check that the right number of queue levels are created (+1 because 0 is a priority level itself)
        BOOST_CHECK_EQUAL(cluster->getqueue()->size(),
                          (uint32_t) Message::Priority::Lowest - (uint32_t) Message::Priority::Highest + 1);

        // Cleanup
        delete cluster;
        delete manager;
        delete details;
    }

    BOOST_AUTO_TEST_CASE(test_getName) {
        // Parse the cluster configuration
        auto jsonClusters = nlohmann::json::parse(sClusters);

        // Get the details for the first cluster
        auto details = new sClusterDetails(jsonClusters[0]);

        // Create a new cluster manager
        auto manager = new ClusterManager();

        // Create a new cluster
        auto cluster = new Cluster(details, manager);

        // Check that the name is correctly set
        BOOST_CHECK_EQUAL(cluster->getName(), details->getName());

        // Cleanup
        delete cluster;
        delete manager;
        delete details;
    }

    BOOST_AUTO_TEST_CASE(test_getClusterDetails) {
        // Parse the cluster configuration
        auto jsonClusters = nlohmann::json::parse(sClusters);

        // Get the details for the first cluster
        auto details = new sClusterDetails(jsonClusters[0]);

        // Create a new cluster manager
        auto manager = new ClusterManager();

        // Create a new cluster
        auto cluster = new Cluster(details, manager);

        // Check that the cluster details are correctly set
        BOOST_CHECK_EQUAL(cluster->getClusterDetails(), details);

        // Cleanup
        delete cluster;
        delete manager;
        delete details;
    }

    BOOST_AUTO_TEST_CASE(test_setConnection) {
        // Parse the cluster configuration
        auto jsonClusters = nlohmann::json::parse(sClusters);

        // Get the details for the first cluster
        auto details = new sClusterDetails(jsonClusters[0]);

        // Create a new cluster manager
        auto manager = new ClusterManager();

        // Create a new cluster
        auto cluster = new Cluster(details, manager);

        // Check that the connection is correctly set
        auto con = new WsServer::Connection(nullptr);
        cluster->setConnection(con);
        BOOST_CHECK_EQUAL(*cluster->getpConnection(), con);

        // Cleanup
        delete cluster;
        delete manager;
        delete details;
        delete con;
    }

    BOOST_AUTO_TEST_CASE(test_isOnline) {
        // Parse the cluster configuration
        auto jsonClusters = nlohmann::json::parse(sClusters);

        // Get the details for the first cluster
        auto details = new sClusterDetails(jsonClusters[0]);

        // Create a new cluster manager
        auto manager = new ClusterManager();

        // Create a new cluster
        auto cluster = new Cluster(details, manager);

        // The cluster should not be online
        BOOST_CHECK_EQUAL(cluster->isOnline(), false);

        // After the connection is set, the cluster should be online
        auto con = new WsServer::Connection(nullptr);
        cluster->setConnection(con);
        BOOST_CHECK_EQUAL(cluster->isOnline(), true);

        // Cleanup
        delete cluster;
        delete manager;
        delete details;
        delete con;
    }

    BOOST_AUTO_TEST_CASE(test_queueMessage) {
        // Parse the cluster configuration
        auto jsonClusters = nlohmann::json::parse(sClusters);

        // Get the details for the first cluster
        auto details = new sClusterDetails(jsonClusters[0]);

        // Create a new cluster manager
        auto manager = new ClusterManager();

        // Create a new cluster
        auto cluster = new Cluster(details, manager);

        // Check the source doesn't exist
        BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Highest].find("s1") ==
                          (*cluster->getqueue())[Message::Priority::Highest].end(), true);

        auto s1_d1 = generateRandomData(randomInt(0, 255));
        cluster->queueMessage("s1", s1_d1, Message::Priority::Highest);

        auto s2_d1 = generateRandomData(randomInt(0, 255));
        cluster->queueMessage("s2", s2_d1, Message::Priority::Lowest);

        auto s3_d1 = generateRandomData(randomInt(0, 255));
        cluster->queueMessage("s3", s3_d1, Message::Priority::Lowest);

        // s1 should only exist in the highest priority queue
        BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Highest].find("s1") ==
                          (*cluster->getqueue())[Message::Priority::Highest].end(), false);
        BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Medium].find("s1") ==
                          (*cluster->getqueue())[Message::Priority::Medium].end(), true);
        BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Lowest].find("s1") ==
                          (*cluster->getqueue())[Message::Priority::Lowest].end(), true);

        // s2 should only exist in the lowest priority queue
        BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Highest].find("s2") ==
                          (*cluster->getqueue())[Message::Priority::Highest].end(), true);
        BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Medium].find("s2") ==
                          (*cluster->getqueue())[Message::Priority::Medium].end(), true);
        BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Lowest].find("s2") ==
                          (*cluster->getqueue())[Message::Priority::Lowest].end(), false);

        // s3 should only exist in the lowest priority queue
        BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Highest].find("s3") ==
                          (*cluster->getqueue())[Message::Priority::Highest].end(), true);
        BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Medium].find("s3") ==
                          (*cluster->getqueue())[Message::Priority::Medium].end(), true);
        BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Lowest].find("s3") ==
                          (*cluster->getqueue())[Message::Priority::Lowest].end(), false);

        auto find_s1 = (*cluster->getqueue())[Message::Priority::Highest].find("s1");
        // s1 should have been put in the queue exactly once
        BOOST_CHECK_EQUAL(find_s1->second->size(), 1);
        // The found s1 should exactly equal s1_d1
        BOOST_CHECK_EQUAL_COLLECTIONS((*find_s1->second->try_peek())->begin(), (*find_s1->second->try_peek())->end(),
                                      s1_d1->begin(), s1_d1->end());

        auto find_s2 = (*cluster->getqueue())[Message::Priority::Lowest].find("s2");
        // s2 should have been put in the queue exactly once
        BOOST_CHECK_EQUAL(find_s2->second->size(), 1);
        // The found s2 should exactly equal s2_d1
        BOOST_CHECK_EQUAL_COLLECTIONS((*find_s2->second->try_peek())->begin(), (*find_s2->second->try_peek())->end(),
                                      s2_d1->begin(), s2_d1->end());

        auto find_s3 = (*cluster->getqueue())[Message::Priority::Lowest].find("s3");
        // s2 should have been put in the queue exactly once
        BOOST_CHECK_EQUAL(find_s3->second->size(), 1);
        // The found s2 should exactly equal s2_d1
        BOOST_CHECK_EQUAL_COLLECTIONS((*find_s3->second->try_peek())->begin(), (*find_s3->second->try_peek())->end(),
                                      s3_d1->begin(), s3_d1->end());

        auto s1_d2 = generateRandomData(randomInt(0, 255));
        cluster->queueMessage("s1", s1_d2, Message::Priority::Highest);
        // s1 should 2 items
        BOOST_CHECK_EQUAL(find_s1->second->size(), 2);

        auto s1_d3 = generateRandomData(randomInt(0, 255));
        cluster->queueMessage("s1", s1_d3, Message::Priority::Highest);

        // s1 should have 3 items
        BOOST_CHECK_EQUAL(find_s1->second->size(), 3);

        // Test dequeuing gives the correct results
        auto d = find_s1->second->dequeue();
        // d should be a copy of s1_d1, it should not be the same reference
        BOOST_CHECK_EQUAL(d == s1_d1, false);
        BOOST_CHECK_EQUAL_COLLECTIONS(d->begin(), d->end(), s1_d1->begin(), s1_d1->end());
        delete d;

        d = find_s1->second->dequeue();
        BOOST_CHECK_EQUAL_COLLECTIONS(d->begin(), d->end(), s1_d2->begin(), s1_d2->end());
        delete d;

        d = find_s1->second->dequeue();
        BOOST_CHECK_EQUAL_COLLECTIONS(d->begin(), d->end(), s1_d3->begin(), s1_d3->end());
        delete d;

        auto s2_d2 = generateRandomData(randomInt(0, 255));
        cluster->queueMessage("s2", s2_d2, Message::Priority::Lowest);
        // s2 should 2 items
        BOOST_CHECK_EQUAL(find_s2->second->size(), 2);

        auto s2_d3 = generateRandomData(randomInt(0, 255));
        cluster->queueMessage("s2", s2_d3, Message::Priority::Lowest);

        // s2 should have 3 items
        BOOST_CHECK_EQUAL(find_s2->second->size(), 3);

        auto s3_d2 = generateRandomData(randomInt(0, 255));
        cluster->queueMessage("s3", s3_d2, Message::Priority::Lowest);
        // s3 should 2 items
        BOOST_CHECK_EQUAL(find_s3->second->size(), 2);

        auto s3_d3 = generateRandomData(randomInt(0, 255));
        cluster->queueMessage("s3", s3_d3, Message::Priority::Lowest);

        // s3 should have 3 items
        BOOST_CHECK_EQUAL(find_s3->second->size(), 3);

        // Test dequeuing gives the correct results
        d = find_s2->second->dequeue();
        BOOST_CHECK_EQUAL_COLLECTIONS(d->begin(), d->end(), s2_d1->begin(), s2_d1->end());
        delete d;

        d = find_s2->second->dequeue();
        BOOST_CHECK_EQUAL_COLLECTIONS(d->begin(), d->end(), s2_d2->begin(), s2_d2->end());
        delete d;

        d = find_s2->second->dequeue();
        BOOST_CHECK_EQUAL_COLLECTIONS(d->begin(), d->end(), s2_d3->begin(), s2_d3->end());
        delete d;

        d = find_s3->second->dequeue();
        BOOST_CHECK_EQUAL_COLLECTIONS(d->begin(), d->end(), s3_d1->begin(), s3_d1->end());
        delete d;

        d = find_s3->second->dequeue();
        BOOST_CHECK_EQUAL_COLLECTIONS(d->begin(), d->end(), s3_d2->begin(), s3_d2->end());
        delete d;

        d = find_s3->second->dequeue();
        BOOST_CHECK_EQUAL_COLLECTIONS(d->begin(), d->end(), s3_d3->begin(), s3_d3->end());
        delete d;

        // Check that after all data has been dequeued, that s1, s2, and s3 queues are empty
        BOOST_CHECK_EQUAL(find_s1->second->size(), 0);
        BOOST_CHECK_EQUAL(find_s2->second->size(), 0);
        BOOST_CHECK_EQUAL(find_s3->second->size(), 0);
    }

    BOOST_AUTO_TEST_CASE(test_pruneSources) {
        // Parse the cluster configuration
        auto jsonClusters = nlohmann::json::parse(sClusters);

        // Get the details for the first cluster
        auto details = new sClusterDetails(jsonClusters[0]);

        // Create a new cluster manager
        auto manager = new ClusterManager();

        // Create a new cluster
        auto cluster = new Cluster(details, manager);

        // Create several sources and insert data in the queue
        auto s1_d1 = generateRandomData(randomInt(0, 255));
        cluster->queueMessage("s1", s1_d1, Message::Priority::Highest);

        auto s2_d1 = generateRandomData(randomInt(0, 255));
        cluster->queueMessage("s2", s2_d1, Message::Priority::Lowest);

        auto s3_d1 = generateRandomData(randomInt(0, 255));
        cluster->queueMessage("s3", s3_d1, Message::Priority::Lowest);

        // Pruning the sources should not perform any action since all sources have one item in the queue
        cluster->callpruneSources();

        BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Highest].find("s1") ==
                          (*cluster->getqueue())[Message::Priority::Highest].end(), false);
        BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Lowest].find("s2") ==
                          (*cluster->getqueue())[Message::Priority::Lowest].end(), false);
        BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Lowest].find("s3") ==
                          (*cluster->getqueue())[Message::Priority::Lowest].end(), false);

        // Dequeue an item from s2, which will leave s2 with 0 items
        (*cluster->getqueue())[Message::Priority::Lowest].find("s2")->second->dequeue();

        // Now pruning the sources should remove s2, but not s1 or s3
        cluster->callpruneSources();

        BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Highest].find("s1") ==
                          (*cluster->getqueue())[Message::Priority::Highest].end(), false);
        BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Lowest].find("s2") ==
                          (*cluster->getqueue())[Message::Priority::Lowest].end(), true);
        BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Lowest].find("s3") ==
                          (*cluster->getqueue())[Message::Priority::Lowest].end(), false);

        // Dequeue the remaining items from s1 and s3
        (*cluster->getqueue())[Message::Priority::Highest].find("s1")->second->dequeue();
        (*cluster->getqueue())[Message::Priority::Lowest].find("s3")->second->dequeue();

        // Now pruning the sources should remove both s1 and s3
        cluster->callpruneSources();

        // There should now be no items left in the queue
        BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Highest].find("s1") ==
                          (*cluster->getqueue())[Message::Priority::Highest].end(), true);
        BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Lowest].find("s2") ==
                          (*cluster->getqueue())[Message::Priority::Lowest].end(), true);
        BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Lowest].find("s3") ==
                          (*cluster->getqueue())[Message::Priority::Lowest].end(), true);

        BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Highest].size(), 0);
        BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Lowest].size(), 0);
    }

    BOOST_AUTO_TEST_CASE(test_run) {
        // Parse the cluster configuration
        auto jsonClusters = nlohmann::json::parse(sClusters);

        // Get the details for the first cluster
        auto details = new sClusterDetails(jsonClusters[0]);

        // Create a new cluster manager
        auto manager = new ClusterManager();

        // Create a new cluster
        auto cluster = new Cluster(details, manager);

        // Create a websocket server to receive sent messages and set the cluster connection
        // Adapted from https://gitlab.com/eidheim/Simple-WebSocket-Server/-/blob/master/tests/io_test.cpp#L11
        WsServer server;
        server.config.port = 23458;

        auto &wsTest = server.endpoint["^/ws/?$"];

        wsTest.on_open = [&](const std::shared_ptr<WsServer::Connection> &connection) {
            cluster->setConnection(connection.get());
        };

        // Start the server
        std::thread server_thread([&server]() {
            server.start();
        });

        // Wait for the server to start
        std::this_thread::sleep_for(std::chrono::milliseconds(100));

        // Create a websocket client to receive messages from the server
        WsClient wsClient("localhost:23458/ws/");

        std::vector<std::vector<uint8_t>> receivedMessages;

        wsClient.on_message = [&receivedMessages](const std::shared_ptr<WsClient::Connection> &connection,
                                                  const std::shared_ptr<WsClient::InMessage> &in_message) {
            auto data = in_message->string();
            receivedMessages.emplace_back(std::vector<uint8_t>(data.begin(), data.end()));
        };

        std::thread client_thread([&wsClient]() {
            wsClient.start();
        });

        // Wait for the client to connect
        std::this_thread::sleep_for(std::chrono::milliseconds(100));

        // Create several sources and insert data in the queue
        auto s1_d1 = generateRandomData(randomInt(0, 255));
        cluster->queueMessage("s1", s1_d1, Message::Priority::Highest);

        auto s1_d2 = generateRandomData(randomInt(0, 255));
        cluster->queueMessage("s1", s1_d2, Message::Priority::Highest);

        auto s2_d1 = generateRandomData(randomInt(0, 255));
        cluster->queueMessage("s2", s2_d1, Message::Priority::Highest);

        auto s3_d1 = generateRandomData(randomInt(0, 255));
        cluster->queueMessage("s3", s3_d1, Message::Priority::Lowest);

        auto s3_d2 = generateRandomData(randomInt(0, 255));
        cluster->queueMessage("s3", s3_d2, Message::Priority::Lowest);

        auto s3_d3 = generateRandomData(randomInt(0, 255));
        cluster->queueMessage("s3", s3_d3, Message::Priority::Lowest);

        auto s3_d4 = generateRandomData(randomInt(0, 255));
        cluster->queueMessage("s3", s3_d4, Message::Priority::Lowest);

        auto s4_d1 = generateRandomData(randomInt(0, 255));
        cluster->queueMessage("s4", s4_d1, Message::Priority::Lowest);

        auto s4_d2 = generateRandomData(randomInt(0, 255));
        cluster->queueMessage("s4", s4_d2, Message::Priority::Lowest);

        auto s5_d1 = generateRandomData(randomInt(0, 255));
        cluster->queueMessage("s5", s5_d1, Message::Priority::Lowest);

        auto s5_d2 = generateRandomData(randomInt(0, 255));
        cluster->queueMessage("s5", s5_d2, Message::Priority::Lowest);

        auto s6_d1 = generateRandomData(randomInt(0, 255));
        cluster->queueMessage("s6", s6_d1, Message::Priority::Medium);

        auto s6_d2 = generateRandomData(randomInt(0, 255));
        cluster->queueMessage("s6", s6_d2, Message::Priority::Medium);

        auto s6_d3 = generateRandomData(randomInt(0, 255));
        cluster->queueMessage("s6", s6_d3, Message::Priority::Medium);

        *cluster->getdataReady() = true;
        cluster->callrun();

        // Give the websocket threads time to complete
        std::this_thread::sleep_for(std::chrono::milliseconds(100));

        // Check that the data sent was in priority/source order
        // The following order is deterministic - but sensitive.
        BOOST_CHECK_EQUAL_COLLECTIONS(receivedMessages[0].begin(), receivedMessages[0].end(), s2_d1->begin(),
                                      s2_d1->end());
        BOOST_CHECK_EQUAL_COLLECTIONS(receivedMessages[1].begin(), receivedMessages[1].end(), s1_d1->begin(),
                                      s1_d1->end());
        BOOST_CHECK_EQUAL_COLLECTIONS(receivedMessages[2].begin(), receivedMessages[2].end(), s1_d2->begin(),
                                      s1_d2->end());

        BOOST_CHECK_EQUAL_COLLECTIONS(receivedMessages[3].begin(), receivedMessages[3].end(), s6_d1->begin(),
                                      s6_d1->end());
        BOOST_CHECK_EQUAL_COLLECTIONS(receivedMessages[4].begin(), receivedMessages[4].end(), s6_d2->begin(),
                                      s6_d2->end());
        BOOST_CHECK_EQUAL_COLLECTIONS(receivedMessages[5].begin(), receivedMessages[5].end(), s6_d3->begin(),
                                      s6_d3->end());

        BOOST_CHECK_EQUAL_COLLECTIONS(receivedMessages[6].begin(), receivedMessages[6].end(), s4_d1->begin(),
                                      s4_d1->end());
        BOOST_CHECK_EQUAL_COLLECTIONS(receivedMessages[7].begin(), receivedMessages[7].end(), s5_d1->begin(),
                                      s5_d1->end());
        BOOST_CHECK_EQUAL_COLLECTIONS(receivedMessages[8].begin(), receivedMessages[8].end(), s3_d1->begin(),
                                      s3_d1->end());
        BOOST_CHECK_EQUAL_COLLECTIONS(receivedMessages[9].begin(), receivedMessages[9].end(), s4_d2->begin(),
                                      s4_d2->end());
        BOOST_CHECK_EQUAL_COLLECTIONS(receivedMessages[10].begin(), receivedMessages[10].end(), s5_d2->begin(),
                                      s5_d2->end());
        BOOST_CHECK_EQUAL_COLLECTIONS(receivedMessages[11].begin(), receivedMessages[11].end(), s3_d2->begin(),
                                      s3_d2->end());
        BOOST_CHECK_EQUAL_COLLECTIONS(receivedMessages[12].begin(), receivedMessages[12].end(), s3_d3->begin(),
                                      s3_d3->end());
        BOOST_CHECK_EQUAL_COLLECTIONS(receivedMessages[13].begin(), receivedMessages[13].end(), s3_d4->begin(),
                                      s3_d4->end());

        // Stop the client and server websocket connections and threads
        wsClient.stop();
        client_thread.join();

        server.stop();
        server_thread.join();
    }

    BOOST_AUTO_TEST_CASE(test_doesHigherPriorityDataExist) {
        // Parse the cluster configuration
        auto jsonClusters = nlohmann::json::parse(sClusters);

        // Get the details for the first cluster
        auto details = new sClusterDetails(jsonClusters[0]);

        // Create a new cluster manager
        auto manager = new ClusterManager();

        // Create a new cluster
        auto cluster = new Cluster(details, manager);

        // There should be no higher priority data if there is no data
        BOOST_CHECK_EQUAL(cluster->calldoesHigherPriorityDataExist((uint64_t) Message::Priority::Highest), false);
        BOOST_CHECK_EQUAL(cluster->calldoesHigherPriorityDataExist((uint64_t) Message::Priority::Lowest), false);

        // Insert some data
        auto s4_d1 = generateRandomData(randomInt(0, 255));
        cluster->queueMessage("s4", s4_d1, Message::Priority::Lowest);
        BOOST_CHECK_EQUAL(cluster->calldoesHigherPriorityDataExist((uint64_t) Message::Priority::Highest), false);
        BOOST_CHECK_EQUAL(cluster->calldoesHigherPriorityDataExist((uint64_t) Message::Priority::Medium), false);
        BOOST_CHECK_EQUAL(cluster->calldoesHigherPriorityDataExist((uint64_t) Message::Priority::Lowest), false);

        auto s3_d1 = generateRandomData(randomInt(0, 255));
        cluster->queueMessage("s3", s3_d1, Message::Priority::Medium);
        BOOST_CHECK_EQUAL(cluster->calldoesHigherPriorityDataExist((uint64_t) Message::Priority::Highest), false);
        BOOST_CHECK_EQUAL(cluster->calldoesHigherPriorityDataExist((uint64_t) Message::Priority::Medium), false);
        BOOST_CHECK_EQUAL(cluster->calldoesHigherPriorityDataExist((uint64_t) Message::Priority::Lowest), true);

        auto s2_d1 = generateRandomData(randomInt(0, 255));
        cluster->queueMessage("s2", s2_d1, Message::Priority::Highest);
        BOOST_CHECK_EQUAL(cluster->calldoesHigherPriorityDataExist((uint64_t) Message::Priority::Highest), false);
        BOOST_CHECK_EQUAL(cluster->calldoesHigherPriorityDataExist((uint64_t) Message::Priority::Medium), true);
        BOOST_CHECK_EQUAL(cluster->calldoesHigherPriorityDataExist((uint64_t) Message::Priority::Lowest), true);

        auto s1_d1 = generateRandomData(randomInt(0, 255));
        cluster->queueMessage("s1", s1_d1, Message::Priority::Highest);
        auto s0_d1 = generateRandomData(randomInt(0, 255));
        cluster->queueMessage("s0", s0_d1, Message::Priority::Highest);
        BOOST_CHECK_EQUAL(cluster->calldoesHigherPriorityDataExist((uint64_t) Message::Priority::Highest), false);
        BOOST_CHECK_EQUAL(cluster->calldoesHigherPriorityDataExist((uint64_t) Message::Priority::Medium), true);
        BOOST_CHECK_EQUAL(cluster->calldoesHigherPriorityDataExist((uint64_t) Message::Priority::Lowest), true);

        // Clear all data from s2, s1 and s0
        (*cluster->getqueue())[Message::Priority::Highest].find("s2")->second->try_dequeue();
        (*cluster->getqueue())[Message::Priority::Highest].find("s1")->second->try_dequeue();
        (*cluster->getqueue())[Message::Priority::Highest].find("s0")->second->try_dequeue();
        BOOST_CHECK_EQUAL(cluster->calldoesHigherPriorityDataExist((uint64_t) Message::Priority::Highest), false);
        BOOST_CHECK_EQUAL(cluster->calldoesHigherPriorityDataExist((uint64_t) Message::Priority::Medium), false);
        BOOST_CHECK_EQUAL(cluster->calldoesHigherPriorityDataExist((uint64_t) Message::Priority::Lowest), true);

        // Clear data from s3 and s4
        (*cluster->getqueue())[Message::Priority::Medium].find("s3")->second->try_dequeue();
        (*cluster->getqueue())[Message::Priority::Lowest].find("s4")->second->try_dequeue();
        BOOST_CHECK_EQUAL(cluster->calldoesHigherPriorityDataExist((uint64_t) Message::Priority::Highest), false);
        BOOST_CHECK_EQUAL(cluster->calldoesHigherPriorityDataExist((uint64_t) Message::Priority::Medium), false);
        BOOST_CHECK_EQUAL(cluster->calldoesHigherPriorityDataExist((uint64_t) Message::Priority::Lowest), false);

        // Testing a non-standard priority that has a value greater than Lowest should now result in false
        BOOST_CHECK_EQUAL(cluster->calldoesHigherPriorityDataExist((uint64_t) Message::Priority::Lowest + 1), false);
    }

    BOOST_AUTO_TEST_CASE(test_updateJob) {
        // Parse the cluster configuration
        auto jsonClusters = nlohmann::json::parse(sClusters);

        // Get the details for the first cluster
        auto details = new sClusterDetails(jsonClusters[0]);

        // Create a new cluster manager
        auto manager = new ClusterManager();

        // Create a new cluster
        auto cluster = new Cluster(details, manager);

        // First make sure we delete all entries from the job history table
        auto db = MySqlConnector();
        schema::JobserverJobhistory jobHistoryTable;
        schema::JobserverJob jobTable;
        schema::JobserverFiledownload fileDownloadTable;

        db->run(remove_from(fileDownloadTable).unconditionally());
        db->run(remove_from(jobHistoryTable).unconditionally());
        db->run(remove_from(jobTable).unconditionally());

        // Create the new job object
        auto jobId = db->run(
                insert_into(jobTable)
                        .set(
                                jobTable.user = 1,
                                jobTable.parameters = "params1",
                                jobTable.cluster = "test",
                                jobTable.bundle = "whatever",
                                jobTable.application = "test"
                        )
        );

        Message msg(UPDATE_JOB);
        msg.push_uint(jobId);               // jobId
        msg.push_string("running");     // what
        msg.push_uint(200);             // status
        msg.push_string("it's fine");   // details
        cluster->handleMessage(msg);

        // Make sure the job status was placed in the database
        auto historyResults =
                db->run(
                        select(all_of(jobHistoryTable))
                                .from(jobHistoryTable)
                                .unconditionally()
                );

        BOOST_CHECK_EQUAL(historyResults.empty(), false);

        // Get the job status
        auto status = &historyResults.front();
        BOOST_CHECK_EQUAL(status->jobId, jobId);
        BOOST_CHECK_EQUAL(std::chrono::system_clock::now() - status->timestamp.value() < std::chrono::seconds{1}, true);
        BOOST_CHECK_EQUAL(status->what, "running");
        BOOST_CHECK_EQUAL(status->state, 200);
        BOOST_CHECK_EQUAL(status->details, "it's fine");

        msg = Message(UPDATE_JOB);
        msg.push_uint(jobId);               // jobId
        msg.push_string("failed");      // what
        msg.push_uint(500);             // status
        msg.push_string("it died");     // details
        cluster->handleMessage(msg);

        // Make sure the job status was placed in the database
        historyResults =
                db->run(
                        select(all_of(jobHistoryTable))
                                .from(jobHistoryTable)
                                .unconditionally()
                );

        BOOST_CHECK_EQUAL(historyResults.empty(), false);

        // Get the job status
        status = &historyResults.front();
        BOOST_CHECK_EQUAL(status->jobId, jobId);
        BOOST_CHECK_EQUAL(std::chrono::system_clock::now() - status->timestamp.value() < std::chrono::seconds{1}, true);
        BOOST_CHECK_EQUAL(status->what, "running");
        BOOST_CHECK_EQUAL(status->state, 200);
        BOOST_CHECK_EQUAL(status->details, "it's fine");

        historyResults.pop_front();
        status = &historyResults.front();
        BOOST_CHECK_EQUAL(status->jobId, jobId);
        BOOST_CHECK_EQUAL(std::chrono::system_clock::now() - status->timestamp.value() < std::chrono::seconds{1}, true);
        BOOST_CHECK_EQUAL(status->what, "failed");
        BOOST_CHECK_EQUAL(status->state, 500);
        BOOST_CHECK_EQUAL(status->details, "it died");

        // Finally clean up all entries from the job history table
        db->run(remove_from(jobHistoryTable).unconditionally());
        db->run(remove_from(jobTable).unconditionally());
    }

    BOOST_AUTO_TEST_CASE(test_checkUnsubmittedJobs) {
        // Parse the cluster configuration
        auto jsonClusters = nlohmann::json::parse(sClusters);

        // Get the details for the first cluster
        auto details = new sClusterDetails(jsonClusters[0]);

        // Create a new cluster manager
        auto manager = new ClusterManager();

        // Create a new cluster
        auto cluster = new Cluster(details, manager);

        // Bring the cluster online
        auto con = new WsServer::Connection(nullptr);
        cluster->setConnection(con);

        // First make sure we delete all entries from the job history table
        auto db = MySqlConnector();
        schema::JobserverJobhistory jobHistoryTable;
        schema::JobserverJob jobTable;
        db->run(remove_from(jobHistoryTable).unconditionally());
        db->run(remove_from(jobTable).unconditionally());

        // Test PENDING works as expected

        // Create the new job object
        auto jobId = db->run(
                insert_into(jobTable)
                        .set(
                                jobTable.user = 1,
                                jobTable.parameters = "params1",
                                jobTable.cluster = "test",
                                jobTable.bundle = "whatever",
                                jobTable.application = "test"
                        )
        );

        // Create the first state object
        db->run(
                insert_into(jobHistoryTable)
                        .set(
                                jobHistoryTable.jobId = jobId,
                                jobHistoryTable.timestamp = std::chrono::system_clock::now(),
                                jobHistoryTable.what = "system",
                                jobHistoryTable.state = (uint32_t) JobStatus::PENDING,
                                jobHistoryTable.details = "Job submitting"
                        )
        );

        // There are no jobs in pending state older than 1 minute
        cluster->callresendMessages();
        BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Medium].size(), 0);

        // Delete all job histories and create a new one pending 59 seconds ago
        db->run(remove_from(jobHistoryTable).unconditionally());
        db->run(
                insert_into(jobHistoryTable)
                        .set(
                                jobHistoryTable.jobId = jobId,
                                jobHistoryTable.timestamp = std::chrono::system_clock::now() - std::chrono::seconds{59},
                                jobHistoryTable.what = "system",
                                jobHistoryTable.state = (uint32_t) JobStatus::PENDING,
                                jobHistoryTable.details = "Job submitting"
                        )
        );

        // There are no jobs in pending state older than 1 minute
        cluster->callresendMessages();
        BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Medium].size(), 0);

        // Delete all job histories and create a new one pending 60 seconds ago
        db->run(remove_from(jobHistoryTable).unconditionally());
        db->run(
                insert_into(jobHistoryTable)
                        .set(
                                jobHistoryTable.jobId = jobId,
                                jobHistoryTable.timestamp = std::chrono::system_clock::now() - std::chrono::seconds{60},
                                jobHistoryTable.what = "system",
                                jobHistoryTable.state = (uint32_t) JobStatus::PENDING,
                                jobHistoryTable.details = "Job submitting"
                        )
        );

        // There is one job in pending state older than 1 minute
        cluster->callresendMessages();
        auto source = std::to_string(jobId) + "_test";
        BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Medium].find(source)->second->size(), 1);

        auto msg = Message(**(*cluster->getqueue())[Message::Priority::Medium].find(source)->second->try_dequeue());
        BOOST_CHECK_EQUAL(msg.getId(), SUBMIT_JOB);
        BOOST_CHECK_EQUAL(msg.pop_uint(), jobId);
        BOOST_CHECK_EQUAL(msg.pop_string(), "whatever");
        BOOST_CHECK_EQUAL(msg.pop_string(), "params1");

        // If the cluster is not online, it should be a noop
        cluster->setConnection(nullptr);
        cluster->callresendMessages();
        BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Medium].find(source)->second->size(), 0);

        // setConnection should call checkUnsubmittedJobs
        cluster->setConnection(con);

        BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Medium].find(source)->second->size(), 1);

        msg = Message(**(*cluster->getqueue())[Message::Priority::Medium].find(source)->second->try_dequeue());
        BOOST_CHECK_EQUAL(msg.getId(), SUBMIT_JOB);
        BOOST_CHECK_EQUAL(msg.pop_uint(), jobId);
        BOOST_CHECK_EQUAL(msg.pop_string(), "whatever");
        BOOST_CHECK_EQUAL(msg.pop_string(), "params1");

        // Delete all job histories and create a new one pending 60 seconds ago
        db->run(remove_from(jobHistoryTable).unconditionally());
        db->run(
                insert_into(jobHistoryTable)
                        .set(
                                jobHistoryTable.jobId = jobId,
                                jobHistoryTable.timestamp = std::chrono::system_clock::now() - std::chrono::seconds{60},
                                jobHistoryTable.what = "system",
                                jobHistoryTable.state = (uint32_t) JobStatus::SUBMITTING,
                                jobHistoryTable.details = "Job submitting"
                        )
        );

        // There is one job in pending state older than 1 minute
        cluster->callresendMessages();
        BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Medium].find(source)->second->size(), 1);
        (*cluster->getqueue())[Message::Priority::Medium].find(source)->second->try_dequeue();

        // Test all other job statuses to make sure nothing is incorrectly returned
        std::vector<JobStatus> noop_statuses = {
                JobStatus::SUBMITTED,
                JobStatus::QUEUED,
                JobStatus::RUNNING,
                JobStatus::CANCELLING,
                JobStatus::CANCELLED,
                JobStatus::DELETING,
                JobStatus::DELETED,
                JobStatus::ERROR,
                JobStatus::WALL_TIME_EXCEEDED,
                JobStatus::OUT_OF_MEMORY,
                JobStatus::COMPLETED
        };

        for (auto i : noop_statuses) {
            // Delete all job histories and create a new one pending 60 seconds ago
            db->run(remove_from(jobHistoryTable).unconditionally());
            db->run(
                    insert_into(jobHistoryTable)
                            .set(
                                    jobHistoryTable.jobId = jobId,
                                    jobHistoryTable.timestamp =
                                            std::chrono::system_clock::now() - std::chrono::seconds{60},
                                    jobHistoryTable.what = "system",
                                    jobHistoryTable.state = (uint32_t) i,
                                    jobHistoryTable.details = "Job submitting"
                            )
            );

            // There should be no jobs resubmitted
            cluster->callresendMessages();
            BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Medium].find(source)->second->size(), 0);
        }

        // Finally clean up all entries from the job history table
        db->run(remove_from(jobHistoryTable).unconditionally());
        db->run(remove_from(jobTable).unconditionally());
    }

    BOOST_AUTO_TEST_CASE(test_handleFileError) {
        // Parse the cluster configuration
        auto jsonClusters = nlohmann::json::parse(sClusters);

        // Get the details for the first cluster
        auto details = new sClusterDetails(jsonClusters[0]);

        // Create a new cluster manager
        auto manager = new ClusterManager();

        // Create a new cluster
        auto cluster = new Cluster(details, manager);

        // A uuid that isn't in the fileDownloadMap should be a noop
        auto uuid = boost::lexical_cast<std::string>(boost::uuids::random_generator()());

        Message msg(FILE_ERROR);
        msg.push_string(uuid);          // uuid
        msg.push_string("details");  // detail
        cluster->handleMessage(msg);

        BOOST_CHECK_EQUAL(fileDownloadMap.size(), 0);

        auto fdObj = new sFileDownload{};
        fileDownloadMap.emplace(uuid, fdObj);

        // Check that the file download object is correctly created
        BOOST_CHECK_EQUAL(fileDownloadMap[uuid]->fileSize, -1);
        BOOST_CHECK_EQUAL(fileDownloadMap[uuid]->error, false);
        BOOST_CHECK_EQUAL(fileDownloadMap[uuid]->errorDetails.empty(), true);
        BOOST_CHECK_EQUAL(fileDownloadMap[uuid]->dataReady, false);
        BOOST_CHECK_EQUAL(fileDownloadMap[uuid]->receivedData, false);
        BOOST_CHECK_EQUAL(fileDownloadMap[uuid]->receivedBytes, 0);
        BOOST_CHECK_EQUAL(fileDownloadMap[uuid]->sentBytes, 0);
        BOOST_CHECK_EQUAL(fileDownloadMap[uuid]->clientPaused, false);

        msg = Message(FILE_ERROR);
        msg.push_string(uuid);          // uuid
        msg.push_string("details");     // detail
        cluster->handleMessage(msg);

        BOOST_CHECK_EQUAL(fileDownloadMap[uuid]->fileSize, -1);
        BOOST_CHECK_EQUAL(fileDownloadMap[uuid]->error, true);
        BOOST_CHECK_EQUAL(fileDownloadMap[uuid]->errorDetails, "details");
        BOOST_CHECK_EQUAL(fileDownloadMap[uuid]->dataReady, true);
        BOOST_CHECK_EQUAL(fileDownloadMap[uuid]->receivedData, false);
        BOOST_CHECK_EQUAL(fileDownloadMap[uuid]->receivedBytes, 0);
        BOOST_CHECK_EQUAL(fileDownloadMap[uuid]->sentBytes, 0);
        BOOST_CHECK_EQUAL(fileDownloadMap[uuid]->clientPaused, false);

        fileDownloadMap.erase(uuid);
    }

    BOOST_AUTO_TEST_CASE(test_handleFileDetails) {
        // Parse the cluster configuration
        auto jsonClusters = nlohmann::json::parse(sClusters);

        // Get the details for the first cluster
        auto details = new sClusterDetails(jsonClusters[0]);

        // Create a new cluster manager
        auto manager = new ClusterManager();

        // Create a new cluster
        auto cluster = new Cluster(details, manager);

        // A uuid that isn't in the fileDownloadMap should be a noop
        auto uuid = boost::lexical_cast<std::string>(boost::uuids::random_generator()());

        Message msg(FILE_DETAILS);
        msg.push_string(uuid);                  // uuid
        msg.push_ulong(0x123456789abcdef);      // fileSize
        cluster->handleMessage(msg);

        BOOST_CHECK_EQUAL(fileDownloadMap.size(), 0);

        auto fdObj = new sFileDownload{};
        fileDownloadMap.emplace(uuid, fdObj);

        // Check that the file download object is correctly created
        BOOST_CHECK_EQUAL(fileDownloadMap[uuid]->fileSize, -1);
        BOOST_CHECK_EQUAL(fileDownloadMap[uuid]->error, false);
        BOOST_CHECK_EQUAL(fileDownloadMap[uuid]->errorDetails.empty(), true);
        BOOST_CHECK_EQUAL(fileDownloadMap[uuid]->dataReady, false);
        BOOST_CHECK_EQUAL(fileDownloadMap[uuid]->receivedData, false);
        BOOST_CHECK_EQUAL(fileDownloadMap[uuid]->receivedBytes, 0);
        BOOST_CHECK_EQUAL(fileDownloadMap[uuid]->sentBytes, 0);
        BOOST_CHECK_EQUAL(fileDownloadMap[uuid]->clientPaused, false);

        msg = Message(FILE_DETAILS);
        msg.push_string(uuid);                  // uuid
        msg.push_ulong(0x123456789abcdef);      // fileSize
        cluster->handleMessage(msg);

        BOOST_CHECK_EQUAL(fileDownloadMap[uuid]->fileSize, 0x123456789abcdef);
        BOOST_CHECK_EQUAL(fileDownloadMap[uuid]->error, false);
        BOOST_CHECK_EQUAL(fileDownloadMap[uuid]->errorDetails.empty(), true);
        BOOST_CHECK_EQUAL(fileDownloadMap[uuid]->dataReady, true);
        BOOST_CHECK_EQUAL(fileDownloadMap[uuid]->receivedData, true);
        BOOST_CHECK_EQUAL(fileDownloadMap[uuid]->receivedBytes, 0);
        BOOST_CHECK_EQUAL(fileDownloadMap[uuid]->sentBytes, 0);
        BOOST_CHECK_EQUAL(fileDownloadMap[uuid]->clientPaused, false);

        fileDownloadMap.erase(uuid);
    }

    BOOST_AUTO_TEST_CASE(test_handleFileChunk) {
        // Parse the cluster configuration
        auto jsonClusters = nlohmann::json::parse(sClusters);

        // Get the details for the first cluster
        auto details = new sClusterDetails(jsonClusters[0]);

        // Create a new cluster manager
        auto manager = new ClusterManager();

        // Create a new cluster
        auto cluster = new Cluster(details, manager);

        // A uuid that isn't in the fileDownloadMap should be a noop
        auto uuid = boost::lexical_cast<std::string>(boost::uuids::random_generator()());

        auto chunk = generateRandomData(randomInt(0, 255));

        Message msg(FILE_CHUNK);
        msg.push_string(uuid);      // uuid
        msg.push_bytes(*chunk);     // chunk
        cluster->handleMessage(msg);

        BOOST_CHECK_EQUAL(fileDownloadMap.size(), 0);

        auto fdObj = new sFileDownload{};
        fileDownloadMap.emplace(uuid, fdObj);

        // Check that the file download object is correctly created
        BOOST_CHECK_EQUAL(fileDownloadMap[uuid]->fileSize, -1);
        BOOST_CHECK_EQUAL(fileDownloadMap[uuid]->error, false);
        BOOST_CHECK_EQUAL(fileDownloadMap[uuid]->errorDetails.empty(), true);
        BOOST_CHECK_EQUAL(fileDownloadMap[uuid]->dataReady, false);
        BOOST_CHECK_EQUAL(fileDownloadMap[uuid]->receivedData, false);
        BOOST_CHECK_EQUAL(fileDownloadMap[uuid]->receivedBytes, 0);
        BOOST_CHECK_EQUAL(fileDownloadMap[uuid]->sentBytes, 0);
        BOOST_CHECK_EQUAL(fileDownloadMap[uuid]->clientPaused, false);

        msg = Message(FILE_CHUNK);
        msg.push_string(uuid);      // uuid
        msg.push_bytes(*chunk);     // chunk
        cluster->handleMessage(msg);

        BOOST_CHECK_EQUAL(fileDownloadMap[uuid]->fileSize, -1);
        BOOST_CHECK_EQUAL(fileDownloadMap[uuid]->error, false);
        BOOST_CHECK_EQUAL(fileDownloadMap[uuid]->errorDetails.empty(), true);
        BOOST_CHECK_EQUAL(fileDownloadMap[uuid]->dataReady, true);
        BOOST_CHECK_EQUAL(fileDownloadMap[uuid]->receivedData, false);
        BOOST_CHECK_EQUAL(fileDownloadMap[uuid]->receivedBytes, chunk->size());
        BOOST_CHECK_EQUAL(fileDownloadMap[uuid]->sentBytes, 0);
        BOOST_CHECK_EQUAL(fileDownloadMap[uuid]->clientPaused, false);

        BOOST_CHECK_EQUAL_COLLECTIONS(chunk->begin(), chunk->end(), (*fileDownloadMap[uuid]->queue.try_peek())->begin(),
                                      (*fileDownloadMap[uuid]->queue.try_peek())->end());

        std::vector<std::vector<uint8_t> *> chunks = {chunk};

        // Fill the queue and make sure that a pause file chunk stream isn't sent until the queue is full
        while (!fileDownloadMap[uuid]->clientPaused) {
            chunk = generateRandomData(randomInt(0, 255));
            chunks.push_back(chunk);

            if ((*cluster->getqueue())[Message::Priority::Highest].size() != 0) {
                BOOST_ASSERT("PAUSE_FILE_CHUNK_STREAM was sent before it should have been");
            }

            msg = Message(FILE_CHUNK);
            msg.push_string(uuid);      // uuid
            msg.push_bytes(*chunk);     // chunk
            cluster->handleMessage(msg);
        }

        // Check that a pause file chunk stream message was sent
        BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Highest].size(), 1);
        msg = Message(**(*cluster->getqueue())[Message::Priority::Highest].find(uuid)->second->try_dequeue());
        BOOST_CHECK_EQUAL(msg.getId(), PAUSE_FILE_CHUNK_STREAM);
        BOOST_CHECK_EQUAL(msg.pop_string(), uuid);

        // Verify that the chunks were correctly queued
        for (auto c : chunks) {
            auto q = (*fileDownloadMap[uuid]->queue.try_dequeue());
            BOOST_CHECK_EQUAL_COLLECTIONS(c->begin(), c->end(), q->begin(), q->end());

            delete c;
            delete q;
        }

        fileDownloadMap.erase(uuid);
    }

    BOOST_AUTO_TEST_CASE(test_handleFileList) {
        // Parse the cluster configuration
        auto jsonClusters = nlohmann::json::parse(sClusters);

        // Get the details for the first cluster
        auto details = new sClusterDetails(jsonClusters[0]);

        // Create a new cluster manager
        auto manager = new ClusterManager();

        // Create a new cluster
        auto cluster = new Cluster(details, manager);

        // A uuid that isn't in the fileListMap should be a noop
        auto uuid = boost::lexical_cast<std::string>(boost::uuids::random_generator()());

        Message msg(FILE_LIST);
        msg.push_string(uuid);      // uuid
        msg.push_uint(3);           // numFiles
        // Directory 1
        msg.push_string("/");       // fileName
        msg.push_bool(true);        // isDirectory
        msg.push_ulong(0);          // fileSize
        // File 1
        msg.push_string("/file1");  // fileName
        msg.push_bool(false);       // isDirectory
        msg.push_ulong(0x1234);     // fileSize
        // File 2
        msg.push_string("/file2");  // fileName
        msg.push_bool(false);       // isDirectory
        msg.push_ulong(0x4321);     // fileSize
        cluster->handleMessage(msg);

        BOOST_CHECK_EQUAL(fileListMap.size(), 0);

        auto fdObj = new sFileList{};
        fileListMap.emplace(uuid, fdObj);

        // Check that the file list object is correctly created
        BOOST_CHECK_EQUAL(fileListMap[uuid]->files.empty(), true);
        BOOST_CHECK_EQUAL(fileListMap[uuid]->error, false);
        BOOST_CHECK_EQUAL(fileListMap[uuid]->errorDetails.empty(), true);
        BOOST_CHECK_EQUAL(fileListMap[uuid]->dataReady, false);

        msg = Message(FILE_LIST);
        msg.push_string(uuid);      // uuid
        msg.push_uint(3);           // numFiles
        // Directory 1
        msg.push_string("/");       // fileName
        msg.push_bool(true);        // isDirectory
        msg.push_ulong(0);          // fileSize
        // File 1
        msg.push_string("/file1");  // fileName
        msg.push_bool(false);       // isDirectory
        msg.push_ulong(0x1234);     // fileSize
        // File 2
        msg.push_string("/file2");  // fileName
        msg.push_bool(false);       // isDirectory
        msg.push_ulong(0x4321);     // fileSize
        cluster->handleMessage(msg);

        BOOST_CHECK_EQUAL(fileListMap[uuid]->files.size(), 3);
        BOOST_CHECK_EQUAL(fileListMap[uuid]->error, false);
        BOOST_CHECK_EQUAL(fileListMap[uuid]->errorDetails.empty(), true);
        BOOST_CHECK_EQUAL(fileListMap[uuid]->dataReady, true);

        BOOST_CHECK_EQUAL(fileListMap[uuid]->files[0].fileName, "/");
        BOOST_CHECK_EQUAL(fileListMap[uuid]->files[0].isDirectory, true);
        BOOST_CHECK_EQUAL(fileListMap[uuid]->files[0].fileSize, 0);

        BOOST_CHECK_EQUAL(fileListMap[uuid]->files[1].fileName, "/file1");
        BOOST_CHECK_EQUAL(fileListMap[uuid]->files[1].isDirectory, false);
        BOOST_CHECK_EQUAL(fileListMap[uuid]->files[1].fileSize, 0x1234);

        BOOST_CHECK_EQUAL(fileListMap[uuid]->files[2].fileName, "/file2");
        BOOST_CHECK_EQUAL(fileListMap[uuid]->files[2].isDirectory, false);
        BOOST_CHECK_EQUAL(fileListMap[uuid]->files[2].fileSize, 0x4321);

        fileListMap.erase(uuid);

    }

BOOST_AUTO_TEST_SUITE_END()
