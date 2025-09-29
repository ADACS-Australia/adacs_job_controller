//
// Created by lewis on 2/10/20.
//

import job_status;
import settings;
#include <random>
#include <utility>

#include <boost/lexical_cast.hpp>
#include <boost/test/unit_test.hpp>
#include <boost/uuid/random_generator.hpp>
#include <boost/uuid/uuid_io.hpp>
#include <jwt/jwt.hpp>
#include <sqlpp11/sqlpp11.h>

#include "../../Lib/FileTypes.h"
#include "../../Lib/shims/sqlpp_shim.h"
#include "../../tests/fixtures/DatabaseFixture.h"
#include "../../tests/fixtures/HttpClientFixture.h"
#include "../../tests/fixtures/WebSocketClientFixture.h"

import ICluster;
import Cluster;
import Message;
import IApplication;
import ClusterManager;

// NOLINTBEGIN(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers,readability-function-cognitive-complexity)
struct ClusterTestDataFixture : public DatabaseFixture, public WebSocketClientFixture, public HttpClientFixture
{
    std::vector<std::vector<uint8_t>> receivedMessages;
    bool bReady = false;
    nlohmann::json jsonClusters;
    std::shared_ptr<sClusterDetails> details;
    std::shared_ptr<Cluster> cluster;
    std::shared_ptr<sClusterDetails> onlineDetails;
    std::shared_ptr<Cluster> onlineCluster;
    uint64_t jobId;
    uint64_t jobIdTestCluster;

    ClusterTestDataFixture()
        : cluster(std::static_pointer_cast<ClusterManager>(clusterManager)->getvClusters()->back()),
          details(
              std::static_pointer_cast<ClusterManager>(clusterManager)->getvClusters()->back()->getClusterDetails()),
          onlineCluster(std::static_pointer_cast<ClusterManager>(clusterManager)->getvClusters()->front()),
          onlineDetails(
              std::static_pointer_cast<ClusterManager>(clusterManager)->getvClusters()->front()->getClusterDetails())
    {
        // Parse the cluster configuration
        jsonClusters = nlohmann::json::parse(sClusters);

        // Create a new job object
        jobId = database->run(insert_into(jobTable).set(jobTable.user        = 1,
                                                        jobTable.parameters  = "params1",
                                                        jobTable.cluster     = cluster->getName(),
                                                        jobTable.bundle      = "whatever",
                                                        jobTable.application = "test"));

        jobIdTestCluster = database->run(insert_into(jobTable).set(jobTable.user        = 1,
                                                                   jobTable.parameters  = "params1",
                                                                   jobTable.cluster     = "testclusternotreal",
                                                                   jobTable.bundle      = "whatever",
                                                                   jobTable.application = "test"));

        websocketClient->on_message = [&]([[maybe_unused]] auto connection, auto in_message) {
            onWebsocketMessage(in_message);
        };

        startWebSocketClient();

        // Wait for the client to connect
        while (!bReady)
        {
            // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
            std::this_thread::sleep_for(std::chrono::milliseconds(10));
        }

        // Clusters should be stopped
        cluster->stop();
        onlineCluster->stop();
    }

    void onWebsocketMessage(auto in_message)
    {
        auto data = in_message->string();
        std::cout << "DEBUG: Received WebSocket message, size: " << data.size() << std::endl;

        // Don't parse the message if the ws connection is ready
        if (!bReady)
        {
            Message msg(std::vector<uint8_t>(data.begin(), data.end()));
            std::cout << "DEBUG: Parsed message, ID: " << msg.getId() << ", SERVER_READY: " << SERVER_READY
                      << std::endl;
            if (msg.getId() == SERVER_READY)
            {
                std::cout << "DEBUG: Received SERVER_READY message, setting bReady = true" << std::endl;
                bReady = true;
                return;
            }
        }

        receivedMessages.emplace_back(std::vector<uint8_t>(data.begin(), data.end()));
    }
};


BOOST_FIXTURE_TEST_SUITE(Cluster_test_suite, ClusterTestDataFixture)

BOOST_AUTO_TEST_CASE(test_constructor)
{
    // Check that the cluster details and the manager are set correctly
    BOOST_CHECK_EQUAL(*cluster->getpClusterDetails(), details);

    // Check that the right number of queue levels are created (+1 because 0 is a priority level itself)
    BOOST_CHECK_EQUAL(cluster->getqueue()->size(),
                      static_cast<uint32_t>(Message::Priority::Lowest) -
                          static_cast<uint32_t>(Message::Priority::Highest) + 1);
}

BOOST_AUTO_TEST_CASE(test_getName)
{
    // Check that the name is correctly set
    BOOST_CHECK_EQUAL(cluster->getName(), details->getName());
}

BOOST_AUTO_TEST_CASE(test_getClusterDetails)
{
    // Check that the cluster details are correctly set
    BOOST_CHECK_EQUAL(cluster->getClusterDetails(), details);
}

BOOST_AUTO_TEST_CASE(test_setConnection)
{
    // Check that the connection is correctly set
    auto con = std::make_shared<WsServer::Connection>(nullptr);
    cluster->setConnection(con);
    BOOST_CHECK_EQUAL(*cluster->getpConnection(), con);
}

BOOST_AUTO_TEST_CASE(test_isOnline)
{
    // The cluster should not be online
    BOOST_CHECK_EQUAL(cluster->isOnline(), false);

    // After the connection is set, the cluster should be online
    auto con = std::make_shared<WsServer::Connection>(nullptr);
    cluster->setConnection(con);
    BOOST_CHECK_EQUAL(cluster->isOnline(), true);
}

BOOST_AUTO_TEST_CASE(test_queueMessage)
{
    cluster->stop();

    // Check the source doesn't exist
    BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Highest].find("s1") ==
                          (*cluster->getqueue())[Message::Priority::Highest].end(),
                      true);

    auto s1_d1 = generateRandomData(randomInt(0, 255));
    cluster->queueMessage("s1", s1_d1, Message::Priority::Highest);

    auto s2_d1 = generateRandomData(randomInt(0, 255));
    cluster->queueMessage("s2", s2_d1, Message::Priority::Lowest);

    auto s3_d1 = generateRandomData(randomInt(0, 255));
    cluster->queueMessage("s3", s3_d1, Message::Priority::Lowest);

    // s1 should only exist in the highest priority queue
    BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Highest].find("s1") ==
                          (*cluster->getqueue())[Message::Priority::Highest].end(),
                      false);
    BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Medium].find("s1") ==
                          (*cluster->getqueue())[Message::Priority::Medium].end(),
                      true);
    BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Lowest].find("s1") ==
                          (*cluster->getqueue())[Message::Priority::Lowest].end(),
                      true);

    // s2 should only exist in the lowest priority queue
    BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Highest].find("s2") ==
                          (*cluster->getqueue())[Message::Priority::Highest].end(),
                      true);
    BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Medium].find("s2") ==
                          (*cluster->getqueue())[Message::Priority::Medium].end(),
                      true);
    BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Lowest].find("s2") ==
                          (*cluster->getqueue())[Message::Priority::Lowest].end(),
                      false);

    // s3 should only exist in the lowest priority queue
    BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Highest].find("s3") ==
                          (*cluster->getqueue())[Message::Priority::Highest].end(),
                      true);
    BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Medium].find("s3") ==
                          (*cluster->getqueue())[Message::Priority::Medium].end(),
                      true);
    BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Lowest].find("s3") ==
                          (*cluster->getqueue())[Message::Priority::Lowest].end(),
                      false);

    auto find_s1 = (*cluster->getqueue())[Message::Priority::Highest].find("s1");
    // s1 should have been put in the queue exactly once
    BOOST_CHECK_EQUAL(find_s1->second->size(), 1);
    // The found s1 should exactly equal s1_d1
    BOOST_CHECK_EQUAL_COLLECTIONS((*find_s1->second->try_peek())->begin(),
                                  (*find_s1->second->try_peek())->end(),
                                  s1_d1->begin(),
                                  s1_d1->end());

    auto find_s2 = (*cluster->getqueue())[Message::Priority::Lowest].find("s2");
    // s2 should have been put in the queue exactly once
    BOOST_CHECK_EQUAL(find_s2->second->size(), 1);
    // The found s2 should exactly equal s2_d1
    BOOST_CHECK_EQUAL_COLLECTIONS((*find_s2->second->try_peek())->begin(),
                                  (*find_s2->second->try_peek())->end(),
                                  s2_d1->begin(),
                                  s2_d1->end());

    auto find_s3 = (*cluster->getqueue())[Message::Priority::Lowest].find("s3");
    // s2 should have been put in the queue exactly once
    BOOST_CHECK_EQUAL(find_s3->second->size(), 1);
    // The found s2 should exactly equal s2_d1
    BOOST_CHECK_EQUAL_COLLECTIONS((*find_s3->second->try_peek())->begin(),
                                  (*find_s3->second->try_peek())->end(),
                                  s3_d1->begin(),
                                  s3_d1->end());

    auto s1_d2 = generateRandomData(randomInt(0, 255));
    cluster->queueMessage("s1", s1_d2, Message::Priority::Highest);
    // s1 should 2 items
    BOOST_CHECK_EQUAL(find_s1->second->size(), 2);

    auto s1_d3 = generateRandomData(randomInt(0, 255));
    cluster->queueMessage("s1", s1_d3, Message::Priority::Highest);

    // s1 should have 3 items
    BOOST_CHECK_EQUAL(find_s1->second->size(), 3);

    // Test dequeuing gives the correct results
    auto data = find_s1->second->dequeue();
    // d should be the same reference as s1_d1
    BOOST_CHECK_EQUAL(data == s1_d1, true);
    BOOST_CHECK_EQUAL_COLLECTIONS(data->begin(), data->end(), s1_d1->begin(), s1_d1->end());

    data = find_s1->second->dequeue();
    BOOST_CHECK_EQUAL_COLLECTIONS(data->begin(), data->end(), s1_d2->begin(), s1_d2->end());

    data = find_s1->second->dequeue();
    BOOST_CHECK_EQUAL_COLLECTIONS(data->begin(), data->end(), s1_d3->begin(), s1_d3->end());

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
    data = find_s2->second->dequeue();
    BOOST_CHECK_EQUAL_COLLECTIONS(data->begin(), data->end(), s2_d1->begin(), s2_d1->end());

    data = find_s2->second->dequeue();
    BOOST_CHECK_EQUAL_COLLECTIONS(data->begin(), data->end(), s2_d2->begin(), s2_d2->end());

    data = find_s2->second->dequeue();
    BOOST_CHECK_EQUAL_COLLECTIONS(data->begin(), data->end(), s2_d3->begin(), s2_d3->end());

    data = find_s3->second->dequeue();
    BOOST_CHECK_EQUAL_COLLECTIONS(data->begin(), data->end(), s3_d1->begin(), s3_d1->end());

    data = find_s3->second->dequeue();
    BOOST_CHECK_EQUAL_COLLECTIONS(data->begin(), data->end(), s3_d2->begin(), s3_d2->end());

    data = find_s3->second->dequeue();
    BOOST_CHECK_EQUAL_COLLECTIONS(data->begin(), data->end(), s3_d3->begin(), s3_d3->end());

    // Check that after all data has been dequeued, that s1, s2, and s3 queues are empty
    BOOST_CHECK_EQUAL(find_s1->second->empty(), true);
    BOOST_CHECK_EQUAL(find_s2->second->empty(), true);
    BOOST_CHECK_EQUAL(find_s3->second->empty(), true);
}

BOOST_AUTO_TEST_CASE(test_pruneSources)
{
    cluster->stop();

    // Create several sources and insert data in the queue
    auto s1_d1 = generateRandomData(randomInt(0, 255));
    cluster->queueMessage("s1", s1_d1, Message::Priority::Highest);

    auto s2_d1 = generateRandomData(randomInt(0, 255));
    cluster->queueMessage("s2", s2_d1, Message::Priority::Lowest);

    auto s3_d1 = generateRandomData(randomInt(0, 255));
    cluster->queueMessage("s3", s3_d1, Message::Priority::Lowest);

    // Pruning the sources should not perform any action since all sources have one item in the queue
    std::promise<void> runPromise;
    auto runThread = std::thread([this, &runPromise] {
        *cluster->getbRunning() = true;
        runPromise.set_value();
        cluster->callpruneSources();
    });

    runPromise.get_future().wait();

    std::this_thread::sleep_for(std::chrono::milliseconds(QUEUE_SOURCE_PRUNE_MILLISECONDS));

    cluster->stop();
    runThread.join();

    BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Highest].find("s1") ==
                          (*cluster->getqueue())[Message::Priority::Highest].end(),
                      false);
    BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Lowest].find("s2") ==
                          (*cluster->getqueue())[Message::Priority::Lowest].end(),
                      false);
    BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Lowest].find("s3") ==
                          (*cluster->getqueue())[Message::Priority::Lowest].end(),
                      false);

    // Dequeue an item from s2, which will leave s2 with 0 items
    (*cluster->getqueue())[Message::Priority::Lowest].find("s2")->second->dequeue();

    // Now pruning the sources should remove s2, but not s1 or s3
    runPromise = std::promise<void>();
    runThread  = std::thread([this, &runPromise] {
        *cluster->getbRunning() = true;
        runPromise.set_value();
        cluster->callpruneSources();
    });

    runPromise.get_future().wait();

    std::this_thread::sleep_for(std::chrono::milliseconds(QUEUE_SOURCE_PRUNE_MILLISECONDS));

    cluster->stop();
    runThread.join();

    BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Highest].find("s1") ==
                          (*cluster->getqueue())[Message::Priority::Highest].end(),
                      false);
    BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Lowest].find("s2") ==
                          (*cluster->getqueue())[Message::Priority::Lowest].end(),
                      true);
    BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Lowest].find("s3") ==
                          (*cluster->getqueue())[Message::Priority::Lowest].end(),
                      false);

    // Dequeue the remaining items from s1 and s3
    (*cluster->getqueue())[Message::Priority::Highest].find("s1")->second->dequeue();
    (*cluster->getqueue())[Message::Priority::Lowest].find("s3")->second->dequeue();

    // Now pruning the sources should remove both s1 and s3
    runPromise = std::promise<void>();
    runThread  = std::thread([this, &runPromise] {
        *cluster->getbRunning() = true;
        runPromise.set_value();
        cluster->callpruneSources();
    });

    runPromise.get_future().wait();

    std::this_thread::sleep_for(std::chrono::milliseconds(QUEUE_SOURCE_PRUNE_MILLISECONDS));

    cluster->stop();
    runThread.join();

    // There should now be no items left in the queue
    BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Highest].find("s1") ==
                          (*cluster->getqueue())[Message::Priority::Highest].end(),
                      true);
    BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Lowest].find("s2") ==
                          (*cluster->getqueue())[Message::Priority::Lowest].end(),
                      true);
    BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Lowest].find("s3") ==
                          (*cluster->getqueue())[Message::Priority::Lowest].end(),
                      true);

    BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Highest].empty(), true);
    BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Lowest].empty(), true);
}

BOOST_AUTO_TEST_CASE(test_run)
{
    stopRunningWebSocket();
    onlineCluster->stop();

    // Create several sources and insert data in the queue
    auto s1_d1 = generateRandomData(randomInt(0, 255));
    onlineCluster->queueMessage("s1", s1_d1, Message::Priority::Highest);

    auto s1_d2 = generateRandomData(randomInt(0, 255));
    onlineCluster->queueMessage("s1", s1_d2, Message::Priority::Highest);

    auto s2_d1 = generateRandomData(randomInt(0, 255));
    onlineCluster->queueMessage("s2", s2_d1, Message::Priority::Highest);

    auto s3_d1 = generateRandomData(randomInt(0, 255));
    onlineCluster->queueMessage("s3", s3_d1, Message::Priority::Lowest);

    auto s3_d2 = generateRandomData(randomInt(0, 255));
    onlineCluster->queueMessage("s3", s3_d2, Message::Priority::Lowest);

    auto s3_d3 = generateRandomData(randomInt(0, 255));
    onlineCluster->queueMessage("s3", s3_d3, Message::Priority::Lowest);

    auto s3_d4 = generateRandomData(randomInt(0, 255));
    onlineCluster->queueMessage("s3", s3_d4, Message::Priority::Lowest);

    auto s4_d1 = generateRandomData(randomInt(0, 255));
    onlineCluster->queueMessage("s4", s4_d1, Message::Priority::Lowest);

    auto s4_d2 = generateRandomData(randomInt(0, 255));
    onlineCluster->queueMessage("s4", s4_d2, Message::Priority::Lowest);

    auto s5_d1 = generateRandomData(randomInt(0, 255));
    onlineCluster->queueMessage("s5", s5_d1, Message::Priority::Lowest);

    auto s5_d2 = generateRandomData(randomInt(0, 255));
    onlineCluster->queueMessage("s5", s5_d2, Message::Priority::Lowest);

    auto s6_d1 = generateRandomData(randomInt(0, 255));
    onlineCluster->queueMessage("s6", s6_d1, Message::Priority::Medium);

    auto s6_d2 = generateRandomData(randomInt(0, 255));
    onlineCluster->queueMessage("s6", s6_d2, Message::Priority::Medium);

    auto s6_d3 = generateRandomData(randomInt(0, 255));
    onlineCluster->queueMessage("s6", s6_d3, Message::Priority::Medium);

    auto runThread = std::thread([this] {
        *onlineCluster->getbRunning()  = true;
        *onlineCluster->getdataReady() = true;
        onlineCluster->callrun();
    });

    // Wait for the messages to be sent
    while (receivedMessages.size() < 14)
    {
        // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
        std::this_thread::sleep_for(std::chrono::milliseconds(10));
    }

    onlineCluster->stop();
    runThread.join();

    // Check that the data sent was in priority/source order
    // The following order is deterministic - but sensitive.
    BOOST_CHECK_EQUAL_COLLECTIONS(receivedMessages[0].begin(), receivedMessages[0].end(), s2_d1->begin(), s2_d1->end());
    BOOST_CHECK_EQUAL_COLLECTIONS(receivedMessages[1].begin(), receivedMessages[1].end(), s1_d1->begin(), s1_d1->end());
    BOOST_CHECK_EQUAL_COLLECTIONS(receivedMessages[2].begin(), receivedMessages[2].end(), s1_d2->begin(), s1_d2->end());

    BOOST_CHECK_EQUAL_COLLECTIONS(receivedMessages[3].begin(), receivedMessages[3].end(), s6_d1->begin(), s6_d1->end());
    BOOST_CHECK_EQUAL_COLLECTIONS(receivedMessages[4].begin(), receivedMessages[4].end(), s6_d2->begin(), s6_d2->end());
    BOOST_CHECK_EQUAL_COLLECTIONS(receivedMessages[5].begin(), receivedMessages[5].end(), s6_d3->begin(), s6_d3->end());

    BOOST_CHECK_EQUAL_COLLECTIONS(receivedMessages[6].begin(), receivedMessages[6].end(), s4_d1->begin(), s4_d1->end());
    BOOST_CHECK_EQUAL_COLLECTIONS(receivedMessages[7].begin(), receivedMessages[7].end(), s5_d1->begin(), s5_d1->end());
    BOOST_CHECK_EQUAL_COLLECTIONS(receivedMessages[8].begin(), receivedMessages[8].end(), s3_d1->begin(), s3_d1->end());
    BOOST_CHECK_EQUAL_COLLECTIONS(receivedMessages[9].begin(), receivedMessages[9].end(), s4_d2->begin(), s4_d2->end());
    BOOST_CHECK_EQUAL_COLLECTIONS(receivedMessages[10].begin(),
                                  receivedMessages[10].end(),
                                  s5_d2->begin(),
                                  s5_d2->end());
    BOOST_CHECK_EQUAL_COLLECTIONS(receivedMessages[11].begin(),
                                  receivedMessages[11].end(),
                                  s3_d2->begin(),
                                  s3_d2->end());
    BOOST_CHECK_EQUAL_COLLECTIONS(receivedMessages[12].begin(),
                                  receivedMessages[12].end(),
                                  s3_d3->begin(),
                                  s3_d3->end());
    BOOST_CHECK_EQUAL_COLLECTIONS(receivedMessages[13].begin(),
                                  receivedMessages[13].end(),
                                  s3_d4->begin(),
                                  s3_d4->end());
}

BOOST_AUTO_TEST_CASE(test_doesHigherPriorityDataExist)
{
    // There should be no higher priority data if there is no data
    BOOST_CHECK_EQUAL(cluster->calldoesHigherPriorityDataExist((uint64_t)Message::Priority::Highest), false);
    BOOST_CHECK_EQUAL(cluster->calldoesHigherPriorityDataExist((uint64_t)Message::Priority::Lowest), false);

    // Insert some data
    auto s4_d1 = generateRandomData(randomInt(0, 255));
    cluster->queueMessage("s4", s4_d1, Message::Priority::Lowest);
    BOOST_CHECK_EQUAL(cluster->calldoesHigherPriorityDataExist((uint64_t)Message::Priority::Highest), false);
    BOOST_CHECK_EQUAL(cluster->calldoesHigherPriorityDataExist((uint64_t)Message::Priority::Medium), false);
    BOOST_CHECK_EQUAL(cluster->calldoesHigherPriorityDataExist((uint64_t)Message::Priority::Lowest), false);

    auto s3_d1 = generateRandomData(randomInt(0, 255));
    cluster->queueMessage("s3", s3_d1, Message::Priority::Medium);
    BOOST_CHECK_EQUAL(cluster->calldoesHigherPriorityDataExist((uint64_t)Message::Priority::Highest), false);
    BOOST_CHECK_EQUAL(cluster->calldoesHigherPriorityDataExist((uint64_t)Message::Priority::Medium), false);
    BOOST_CHECK_EQUAL(cluster->calldoesHigherPriorityDataExist((uint64_t)Message::Priority::Lowest), true);

    auto s2_d1 = generateRandomData(randomInt(0, 255));
    cluster->queueMessage("s2", s2_d1, Message::Priority::Highest);
    BOOST_CHECK_EQUAL(cluster->calldoesHigherPriorityDataExist((uint64_t)Message::Priority::Highest), false);
    BOOST_CHECK_EQUAL(cluster->calldoesHigherPriorityDataExist((uint64_t)Message::Priority::Medium), true);
    BOOST_CHECK_EQUAL(cluster->calldoesHigherPriorityDataExist((uint64_t)Message::Priority::Lowest), true);

    auto s1_d1 = generateRandomData(randomInt(0, 255));
    cluster->queueMessage("s1", s1_d1, Message::Priority::Highest);
    auto s0_d1 = generateRandomData(randomInt(0, 255));
    cluster->queueMessage("s0", s0_d1, Message::Priority::Highest);
    BOOST_CHECK_EQUAL(cluster->calldoesHigherPriorityDataExist((uint64_t)Message::Priority::Highest), false);
    BOOST_CHECK_EQUAL(cluster->calldoesHigherPriorityDataExist((uint64_t)Message::Priority::Medium), true);
    BOOST_CHECK_EQUAL(cluster->calldoesHigherPriorityDataExist((uint64_t)Message::Priority::Lowest), true);

    // Clear all data from s2, s1 and s0
    *(*cluster->getqueue())[Message::Priority::Highest].find("s2")->second->try_dequeue();
    *(*cluster->getqueue())[Message::Priority::Highest].find("s1")->second->try_dequeue();
    *(*cluster->getqueue())[Message::Priority::Highest].find("s0")->second->try_dequeue();
    BOOST_CHECK_EQUAL(cluster->calldoesHigherPriorityDataExist((uint64_t)Message::Priority::Highest), false);
    BOOST_CHECK_EQUAL(cluster->calldoesHigherPriorityDataExist((uint64_t)Message::Priority::Medium), false);
    BOOST_CHECK_EQUAL(cluster->calldoesHigherPriorityDataExist((uint64_t)Message::Priority::Lowest), true);

    // Clear data from s3 and s4
    *(*cluster->getqueue())[Message::Priority::Medium].find("s3")->second->try_dequeue();
    *(*cluster->getqueue())[Message::Priority::Lowest].find("s4")->second->try_dequeue();
    BOOST_CHECK_EQUAL(cluster->calldoesHigherPriorityDataExist((uint64_t)Message::Priority::Highest), false);
    BOOST_CHECK_EQUAL(cluster->calldoesHigherPriorityDataExist((uint64_t)Message::Priority::Medium), false);
    BOOST_CHECK_EQUAL(cluster->calldoesHigherPriorityDataExist((uint64_t)Message::Priority::Lowest), false);

    // Testing a non-standard priority that has a value greater than Lowest should now result in false
    BOOST_CHECK_EQUAL(cluster->calldoesHigherPriorityDataExist((uint64_t)Message::Priority::Lowest + 1), false);
}

BOOST_AUTO_TEST_CASE(test_updateJob)
{
    onlineCluster->stop();

    Message msg(UPDATE_JOB);
    msg.push_uint(jobId);          // jobId
    msg.push_string("running");    // what
    msg.push_uint(200);            // status
    msg.push_string("it's fine");  // details
    cluster->handleMessage(msg);

    // Make sure the job status was placed in the database
    auto historyResults = database->run(select(all_of(jobHistoryTable)).from(jobHistoryTable).unconditionally());

    BOOST_CHECK_EQUAL(historyResults.empty(), false);

    // Get the job status
    const auto* status = &historyResults.front();
    BOOST_CHECK_EQUAL((uint64_t)status->jobId, (uint64_t)jobId);
    BOOST_CHECK_EQUAL(std::chrono::system_clock::now() - status->timestamp.value() < std::chrono::seconds{1}, true);
    BOOST_CHECK_EQUAL(status->what, "running");
    BOOST_CHECK_EQUAL((uint32_t)status->state, (uint32_t)200);
    BOOST_CHECK_EQUAL(status->details, "it's fine");

    msg = Message(UPDATE_JOB);
    msg.push_uint(jobId);        // jobId
    msg.push_string("failed");   // what
    msg.push_uint(500);          // status
    msg.push_string("it died");  // details
    cluster->handleMessage(msg);

    // Make sure the job status was placed in the database
    historyResults = database->run(select(all_of(jobHistoryTable)).from(jobHistoryTable).unconditionally());

    BOOST_CHECK_EQUAL(historyResults.empty(), false);

    // Get the job status
    status = &historyResults.front();
    BOOST_CHECK_EQUAL((uint64_t)status->jobId, (uint64_t)jobId);
    BOOST_CHECK_EQUAL(std::chrono::system_clock::now() - status->timestamp.value() < std::chrono::seconds{1}, true);
    BOOST_CHECK_EQUAL(status->what, "running");
    BOOST_CHECK_EQUAL((uint32_t)status->state, (uint32_t)200);
    BOOST_CHECK_EQUAL(status->details, "it's fine");

    historyResults.pop_front();
    status = &historyResults.front();
    BOOST_CHECK_EQUAL((uint64_t)status->jobId, (uint64_t)jobId);
    BOOST_CHECK_EQUAL(std::chrono::system_clock::now() - status->timestamp.value() < std::chrono::seconds{1}, true);
    BOOST_CHECK_EQUAL(status->what, "failed");
    BOOST_CHECK_EQUAL((uint32_t)status->state, (uint32_t)500);
    BOOST_CHECK_EQUAL(status->details, "it died");
}

BOOST_AUTO_TEST_CASE(test_checkUnsubmittedJobs)
{
    // Bring the cluster online
    auto con = std::make_shared<WsServer::Connection>(nullptr);
    cluster->setConnection(con);

    // Test PENDING works as expected

    // Create the first state object
    database->run(insert_into(jobHistoryTable)
                      .set(jobHistoryTable.jobId     = jobId,
                           jobHistoryTable.timestamp = std::chrono::system_clock::now(),
                           jobHistoryTable.what      = SYSTEM_SOURCE,
                           jobHistoryTable.state     = static_cast<uint32_t>(JobStatus::PENDING),
                           jobHistoryTable.details   = "Job pending"));

    // There are no jobs in pending state older than 1 minute
    cluster->callcheckUnsubmittedJobs();
    BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Medium].empty(), true);

    // Delete all job histories and create a new one pending 59 seconds ago
    database->run(remove_from(jobHistoryTable).unconditionally());
    database->run(insert_into(jobHistoryTable)
                      .set(jobHistoryTable.jobId     = jobId,
                           jobHistoryTable.timestamp = std::chrono::system_clock::now() - std::chrono::seconds{59},
                           jobHistoryTable.what      = SYSTEM_SOURCE,
                           jobHistoryTable.state     = static_cast<uint32_t>(JobStatus::PENDING),
                           jobHistoryTable.details   = "Job pending"));

    // There are no jobs in pending state older than 1 minute
    cluster->callcheckUnsubmittedJobs();
    BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Medium].empty(), true);

    // Delete all job histories and create one with a timestamp at least 60 seconds ago for the job not from
    // this cluster, it should be a noop
    database->run(remove_from(jobHistoryTable).unconditionally());
    database->run(insert_into(jobHistoryTable)
                      .set(jobHistoryTable.jobId     = jobIdTestCluster,
                           jobHistoryTable.timestamp = std::chrono::system_clock::now() - std::chrono::seconds{60},
                           jobHistoryTable.what      = SYSTEM_SOURCE,
                           jobHistoryTable.state     = static_cast<uint32_t>(JobStatus::PENDING),
                           jobHistoryTable.details   = "Job pending"));

    cluster->callcheckUnsubmittedJobs();
    BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Medium].empty(), true);

    // Delete all job histories and create a new one pending 60 seconds ago for a job from this cluster
    database->run(remove_from(jobHistoryTable).unconditionally());
    database->run(insert_into(jobHistoryTable)
                      .set(jobHistoryTable.jobId     = jobId,
                           jobHistoryTable.timestamp = std::chrono::system_clock::now() - std::chrono::seconds{60},
                           jobHistoryTable.what      = SYSTEM_SOURCE,
                           jobHistoryTable.state     = static_cast<uint32_t>(JobStatus::PENDING),
                           jobHistoryTable.details   = "Job pending"));

    // There is one job in pending state older than 1 minute
    cluster->callcheckUnsubmittedJobs();
    auto source = std::to_string(jobId) + "_" + cluster->getName();
    BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Medium].find(source)->second->size(), 1);

    auto ptr = *(*cluster->getqueue())[Message::Priority::Medium].find(source)->second->try_dequeue();
    auto msg = Message(*ptr);

    BOOST_CHECK_EQUAL(msg.getId(), SUBMIT_JOB);
    BOOST_CHECK_EQUAL(msg.pop_uint(), jobId);
    BOOST_CHECK_EQUAL(msg.pop_string(), "whatever");
    BOOST_CHECK_EQUAL(msg.pop_string(), "params1");

    // Check that the job transitioned to submitting state from pending state
    auto jobHistoryResults = database->run(select(all_of(jobHistoryTable))
                                               .from(jobHistoryTable)
                                               .where(jobHistoryTable.jobId == jobId)
                                               .order_by(jobHistoryTable.timestamp.desc())
                                               .limit(1U));

    const auto* dbHistory = &jobHistoryResults.front();
    BOOST_CHECK_EQUAL(dbHistory->what, SYSTEM_SOURCE);
    BOOST_CHECK_EQUAL((uint32_t)dbHistory->state, (uint32_t)JobStatus::SUBMITTING);

    // If the cluster is not online, it should be a noop
    cluster->setConnection(nullptr);
    cluster->callcheckUnsubmittedJobs();
    BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Medium].find(source)->second->empty(), true);

    // setConnection should call checkUnsubmittedJobs
    cluster->setConnection(con);

    BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Medium].find(source)->second->size(), 1);

    ptr = *(*cluster->getqueue())[Message::Priority::Medium].find(source)->second->try_dequeue();
    msg = Message(*ptr);

    BOOST_CHECK_EQUAL(msg.getId(), SUBMIT_JOB);
    BOOST_CHECK_EQUAL(msg.pop_uint(), jobId);
    BOOST_CHECK_EQUAL(msg.pop_string(), "whatever");
    BOOST_CHECK_EQUAL(msg.pop_string(), "params1");

    // Delete all job histories and create a new one submitting 60 seconds ago
    database->run(remove_from(jobHistoryTable).unconditionally());
    database->run(insert_into(jobHistoryTable)
                      .set(jobHistoryTable.jobId     = jobId,
                           jobHistoryTable.timestamp = std::chrono::system_clock::now() - std::chrono::seconds{60},
                           jobHistoryTable.what      = SYSTEM_SOURCE,
                           jobHistoryTable.state     = static_cast<uint32_t>(JobStatus::SUBMITTING),
                           jobHistoryTable.details   = "Job submitting"));

    // There is one job in submitting state older than 1 minute
    cluster->callcheckUnsubmittedJobs();
    BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Medium].find(source)->second->size(), 1);
    *(*cluster->getqueue())[Message::Priority::Medium].find(source)->second->try_dequeue();

    // Check that the job transitioned to submitting state from pending state
    jobHistoryResults = database->run(select(all_of(jobHistoryTable))
                                          .from(jobHistoryTable)
                                          .where(jobHistoryTable.jobId == jobId)
                                          .order_by(jobHistoryTable.timestamp.desc())
                                          .limit(1U));

    dbHistory = &jobHistoryResults.front();
    BOOST_CHECK_EQUAL(dbHistory->what, SYSTEM_SOURCE);
    BOOST_CHECK_EQUAL((uint32_t)dbHistory->state, (uint32_t)JobStatus::SUBMITTING);

    // Test all other job statuses to make sure nothing is incorrectly returned
    std::vector<JobStatus> noop_statuses = {JobStatus::SUBMITTED,
                                            JobStatus::QUEUED,
                                            JobStatus::RUNNING,
                                            JobStatus::CANCELLING,
                                            JobStatus::CANCELLED,
                                            JobStatus::DELETING,
                                            JobStatus::DELETED,
                                            JobStatus::ERROR,
                                            JobStatus::WALL_TIME_EXCEEDED,
                                            JobStatus::OUT_OF_MEMORY,
                                            JobStatus::COMPLETED};

    for (auto status : noop_statuses)
    {
        // Delete all job histories and create a new one pending 60 seconds ago
        database->run(remove_from(jobHistoryTable).unconditionally());
        database->run(insert_into(jobHistoryTable)
                          .set(jobHistoryTable.jobId     = jobId,
                               jobHistoryTable.timestamp = std::chrono::system_clock::now() - std::chrono::seconds{60},
                               jobHistoryTable.what      = SYSTEM_SOURCE,
                               jobHistoryTable.state     = static_cast<uint32_t>(status),
                               jobHistoryTable.details   = "Job submitting"));

        // There should be no jobs resubmitted
        cluster->callcheckUnsubmittedJobs();
        BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Medium].find(source)->second->empty(), true);
    }
}

BOOST_AUTO_TEST_CASE(test_checkCancellingJobs)
{
    // Bring the cluster online
    auto con = std::make_shared<WsServer::Connection>(nullptr);
    cluster->setConnection(con);

    // Test CANCELLING works as expected

    // Create the first state object
    database->run(insert_into(jobHistoryTable)
                      .set(jobHistoryTable.jobId     = jobId,
                           jobHistoryTable.timestamp = std::chrono::system_clock::now(),
                           jobHistoryTable.what      = SYSTEM_SOURCE,
                           jobHistoryTable.state     = static_cast<uint32_t>(JobStatus::CANCELLING),
                           jobHistoryTable.details   = "Job cancelling"));

    // There are no jobs in cancelling state older than 1 minute
    cluster->callcheckCancellingJobs();
    BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Medium].empty(), true);

    // Delete all job histories and create a new one cancelling 59 seconds ago
    database->run(remove_from(jobHistoryTable).unconditionally());
    database->run(insert_into(jobHistoryTable)
                      .set(jobHistoryTable.jobId     = jobId,
                           jobHistoryTable.timestamp = std::chrono::system_clock::now() - std::chrono::seconds{59},
                           jobHistoryTable.what      = SYSTEM_SOURCE,
                           jobHistoryTable.state     = static_cast<uint32_t>(JobStatus::CANCELLING),
                           jobHistoryTable.details   = "Job cancelling"));

    // There are no jobs in cancelling state older than 1 minute
    cluster->callcheckCancellingJobs();
    BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Medium].empty(), true);

    // Delete all job histories and create one with a timestamp at least 60 seconds ago for the job not from
    // this cluster, it should be a noop
    database->run(remove_from(jobHistoryTable).unconditionally());
    database->run(insert_into(jobHistoryTable)
                      .set(jobHistoryTable.jobId     = jobIdTestCluster,
                           jobHistoryTable.timestamp = std::chrono::system_clock::now() - std::chrono::seconds{60},
                           jobHistoryTable.what      = SYSTEM_SOURCE,
                           jobHistoryTable.state     = static_cast<uint32_t>(JobStatus::CANCELLING),
                           jobHistoryTable.details   = "Job pending"));

    cluster->callcheckUnsubmittedJobs();
    BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Medium].empty(), true);

    // Delete all job histories and create a new one cancelling 60 seconds ago
    database->run(remove_from(jobHistoryTable).unconditionally());
    database->run(insert_into(jobHistoryTable)
                      .set(jobHistoryTable.jobId     = jobId,
                           jobHistoryTable.timestamp = std::chrono::system_clock::now() - std::chrono::seconds{60},
                           jobHistoryTable.what      = SYSTEM_SOURCE,
                           jobHistoryTable.state     = static_cast<uint32_t>(JobStatus::CANCELLING),
                           jobHistoryTable.details   = "Job cancelling"));

    // There is one job in cancelling state older than 1 minute
    cluster->callcheckCancellingJobs();
    auto source = std::to_string(jobId) + "_" + cluster->getName();
    BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Medium].find(source)->second->size(), 1);

    auto ptr = *(*cluster->getqueue())[Message::Priority::Medium].find(source)->second->try_dequeue();
    auto msg = Message(*ptr);

    BOOST_CHECK_EQUAL(msg.getId(), CANCEL_JOB);
    BOOST_CHECK_EQUAL(msg.pop_uint(), jobId);

    // If the cluster is not online, it should be a noop
    cluster->setConnection(nullptr);
    cluster->callcheckCancellingJobs();
    BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Medium].find(source)->second->empty(), true);

    // setConnection should call checkUnsubmittedJobs
    cluster->setConnection(con);

    BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Medium].find(source)->second->size(), 1);

    ptr = *(*cluster->getqueue())[Message::Priority::Medium].find(source)->second->try_dequeue();
    msg = Message(*ptr);

    BOOST_CHECK_EQUAL(msg.getId(), CANCEL_JOB);
    BOOST_CHECK_EQUAL(msg.pop_uint(), jobId);

    // Test all other job statuses to make sure nothing is incorrectly returned
    std::vector<JobStatus> noop_statuses = {JobStatus::PENDING,
                                            JobStatus::SUBMITTING,
                                            JobStatus::SUBMITTED,
                                            JobStatus::QUEUED,
                                            JobStatus::RUNNING,
                                            JobStatus::CANCELLED,
                                            JobStatus::DELETING,
                                            JobStatus::DELETED,
                                            JobStatus::ERROR,
                                            JobStatus::WALL_TIME_EXCEEDED,
                                            JobStatus::OUT_OF_MEMORY,
                                            JobStatus::COMPLETED};

    for (auto status : noop_statuses)
    {
        // Delete all job histories and create a new one pending 60 seconds ago
        database->run(remove_from(jobHistoryTable).unconditionally());
        database->run(insert_into(jobHistoryTable)
                          .set(jobHistoryTable.jobId     = jobId,
                               jobHistoryTable.timestamp = std::chrono::system_clock::now() - std::chrono::seconds{60},
                               jobHistoryTable.what      = SYSTEM_SOURCE,
                               jobHistoryTable.state     = static_cast<uint32_t>(status),
                               jobHistoryTable.details   = "Job submitting"));

        // There should be no jobs resubmitted
        cluster->callcheckCancellingJobs();
        BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Medium].find(source)->second->empty(), true);
    }
}

BOOST_AUTO_TEST_CASE(test_checkDeletingJobs)
{
    // Bring the cluster online
    auto con = std::make_shared<WsServer::Connection>(nullptr);
    cluster->setConnection(con);

    // Test DELETING works as expected

    // Create the first state object
    database->run(insert_into(jobHistoryTable)
                      .set(jobHistoryTable.jobId     = jobId,
                           jobHistoryTable.timestamp = std::chrono::system_clock::now(),
                           jobHistoryTable.what      = SYSTEM_SOURCE,
                           jobHistoryTable.state     = static_cast<uint32_t>(JobStatus::DELETING),
                           jobHistoryTable.details   = "Job deleting"));

    // There are no jobs in deleting state older than 1 minute
    cluster->callcheckDeletingJobs();
    BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Medium].empty(), true);

    // Delete all job histories and create a new one deleting 59 seconds ago
    database->run(remove_from(jobHistoryTable).unconditionally());
    database->run(insert_into(jobHistoryTable)
                      .set(jobHistoryTable.jobId     = jobId,
                           jobHistoryTable.timestamp = std::chrono::system_clock::now() - std::chrono::seconds{59},
                           jobHistoryTable.what      = SYSTEM_SOURCE,
                           jobHistoryTable.state     = static_cast<uint32_t>(JobStatus::DELETING),
                           jobHistoryTable.details   = "Job deleting"));

    // There are no jobs in deleting state older than 1 minute
    cluster->callcheckDeletingJobs();
    BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Medium].empty(), true);

    // Delete all job histories and create one with a timestamp at least 60 seconds ago for the job not from
    // this cluster, it should be a noop
    database->run(remove_from(jobHistoryTable).unconditionally());
    database->run(insert_into(jobHistoryTable)
                      .set(jobHistoryTable.jobId     = jobIdTestCluster,
                           jobHistoryTable.timestamp = std::chrono::system_clock::now() - std::chrono::seconds{60},
                           jobHistoryTable.what      = SYSTEM_SOURCE,
                           jobHistoryTable.state     = static_cast<uint32_t>(JobStatus::DELETING),
                           jobHistoryTable.details   = "Job pending"));

    cluster->callcheckUnsubmittedJobs();
    BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Medium].empty(), true);

    // Delete all job histories and create a new one deleting 60 seconds ago
    database->run(remove_from(jobHistoryTable).unconditionally());
    database->run(insert_into(jobHistoryTable)
                      .set(jobHistoryTable.jobId     = jobId,
                           jobHistoryTable.timestamp = std::chrono::system_clock::now() - std::chrono::seconds{60},
                           jobHistoryTable.what      = SYSTEM_SOURCE,
                           jobHistoryTable.state     = static_cast<uint32_t>(JobStatus::DELETING),
                           jobHistoryTable.details   = "Job deleting"));

    // There is one job in deleting state older than 1 minute
    cluster->callcheckDeletingJobs();
    auto source = std::to_string(jobId) + "_" + cluster->getName();
    BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Medium].find(source)->second->size(), 1);

    auto ptr = *(*cluster->getqueue())[Message::Priority::Medium].find(source)->second->try_dequeue();
    auto msg = Message(*ptr);

    BOOST_CHECK_EQUAL(msg.getId(), DELETE_JOB);
    BOOST_CHECK_EQUAL(msg.pop_uint(), jobId);

    // If the cluster is not online, it should be a noop
    cluster->setConnection(nullptr);
    cluster->callcheckDeletingJobs();
    BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Medium].find(source)->second->empty(), true);

    // setConnection should call checkUnsubmittedJobs
    cluster->setConnection(con);

    BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Medium].find(source)->second->size(), 1);

    ptr = *(*cluster->getqueue())[Message::Priority::Medium].find(source)->second->try_dequeue();
    msg = Message(*ptr);

    BOOST_CHECK_EQUAL(msg.getId(), DELETE_JOB);
    BOOST_CHECK_EQUAL(msg.pop_uint(), jobId);

    // Test all other job statuses to make sure nothing is incorrectly returned
    std::vector<JobStatus> noop_statuses = {JobStatus::PENDING,
                                            JobStatus::SUBMITTING,
                                            JobStatus::SUBMITTED,
                                            JobStatus::QUEUED,
                                            JobStatus::RUNNING,
                                            JobStatus::CANCELLING,
                                            JobStatus::CANCELLED,
                                            JobStatus::DELETED,
                                            JobStatus::ERROR,
                                            JobStatus::WALL_TIME_EXCEEDED,
                                            JobStatus::OUT_OF_MEMORY,
                                            JobStatus::COMPLETED};

    for (auto status : noop_statuses)
    {
        // Delete all job histories and create a new one pending 60 seconds ago
        database->run(remove_from(jobHistoryTable).unconditionally());
        database->run(insert_into(jobHistoryTable)
                          .set(jobHistoryTable.jobId     = jobId,
                               jobHistoryTable.timestamp = std::chrono::system_clock::now() - std::chrono::seconds{60},
                               jobHistoryTable.what      = SYSTEM_SOURCE,
                               jobHistoryTable.state     = static_cast<uint32_t>(status),
                               jobHistoryTable.details   = "Job submitting"));

        // There should be no jobs resubmitted
        cluster->callcheckDeletingJobs();
        BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Medium].find(source)->second->empty(), true);
    }
}

BOOST_AUTO_TEST_CASE(test_handleFileList)
{
    // A uuid that isn't in the fileListMap should be a noop
    auto uuid = boost::lexical_cast<std::string>(boost::uuids::random_generator()());

    Message msg(FILE_LIST);
    msg.push_string(uuid);  // uuid
    msg.push_uint(3);       // numFiles
    // Directory 1
    msg.push_string("/");  // fileName
    msg.push_bool(true);   // isDirectory
    msg.push_ulong(0);     // fileSize
    // File 1
    msg.push_string("/file1");  // fileName
    msg.push_bool(false);       // isDirectory
    msg.push_ulong(0x1234);     // fileSize
    // File 2
    msg.push_string("/file2");  // fileName
    msg.push_bool(false);       // isDirectory
    msg.push_ulong(0x4321);     // fileSize
    cluster->handleMessage(msg);

    auto fileListMap = application->getFileListMap();
    BOOST_CHECK_EQUAL(fileListMap->empty(), true);

    auto fdObj = std::make_shared<sFileList>();
    fileListMap->emplace(uuid, fdObj);

    // Check that the file list object is correctly created
    BOOST_CHECK_EQUAL((*fileListMap)[uuid]->files.empty(), true);
    BOOST_CHECK_EQUAL((*fileListMap)[uuid]->error, false);
    BOOST_CHECK_EQUAL((*fileListMap)[uuid]->errorDetails.empty(), true);
    BOOST_CHECK_EQUAL((*fileListMap)[uuid]->dataReady, false);

    msg = Message(FILE_LIST);
    msg.push_string(uuid);  // uuid
    msg.push_uint(3);       // numFiles
    // Directory 1
    msg.push_string("/");  // fileName
    msg.push_bool(true);   // isDirectory
    msg.push_ulong(0);     // fileSize
    // File 1
    msg.push_string("/file1");  // fileName
    msg.push_bool(false);       // isDirectory
    msg.push_ulong(0x1234);     // fileSize
    // File 2
    msg.push_string("/file2");  // fileName
    msg.push_bool(false);       // isDirectory
    msg.push_ulong(0x4321);     // fileSize
    cluster->handleMessage(msg);

    BOOST_CHECK_EQUAL((*fileListMap)[uuid]->files.size(), 3);
    BOOST_CHECK_EQUAL((*fileListMap)[uuid]->error, false);
    BOOST_CHECK_EQUAL((*fileListMap)[uuid]->errorDetails.empty(), true);
    BOOST_CHECK_EQUAL((*fileListMap)[uuid]->dataReady, true);

    BOOST_CHECK_EQUAL((*fileListMap)[uuid]->files[0].fileName, "/");
    BOOST_CHECK_EQUAL((*fileListMap)[uuid]->files[0].isDirectory, true);
    BOOST_CHECK_EQUAL((*fileListMap)[uuid]->files[0].fileSize, 0);

    BOOST_CHECK_EQUAL((*fileListMap)[uuid]->files[1].fileName, "/file1");
    BOOST_CHECK_EQUAL((*fileListMap)[uuid]->files[1].isDirectory, false);
    BOOST_CHECK_EQUAL((*fileListMap)[uuid]->files[1].fileSize, 0x1234);

    BOOST_CHECK_EQUAL((*fileListMap)[uuid]->files[2].fileName, "/file2");
    BOOST_CHECK_EQUAL((*fileListMap)[uuid]->files[2].isDirectory, false);
    BOOST_CHECK_EQUAL((*fileListMap)[uuid]->files[2].fileSize, 0x4321);

    fileListMap->erase(uuid);
}

BOOST_AUTO_TEST_CASE(test_handleFileListError)
{
    // A uuid that isn't in the fileDListMap should be a noop
    auto uuid = boost::lexical_cast<std::string>(boost::uuids::random_generator()());

    Message msg(FILE_LIST_ERROR);
    msg.push_string(uuid);       // uuid
    msg.push_string("details");  // detail
    cluster->handleMessage(msg);

    auto fileListMap2 = application->getFileListMap();
    BOOST_CHECK_EQUAL(fileListMap2->empty(), true);

    auto flObj = std::make_shared<sFileList>();
    fileListMap2->emplace(uuid, flObj);

    // Check that the file download object is correctly created
    BOOST_CHECK_EQUAL((*fileListMap2)[uuid]->files.empty(), true);
    BOOST_CHECK_EQUAL((*fileListMap2)[uuid]->error, false);
    BOOST_CHECK_EQUAL((*fileListMap2)[uuid]->errorDetails.empty(), true);
    BOOST_CHECK_EQUAL((*fileListMap2)[uuid]->dataReady, false);

    msg = Message(FILE_LIST_ERROR);
    msg.push_string(uuid);       // uuid
    msg.push_string("details");  // detail
    cluster->handleMessage(msg);

    BOOST_CHECK_EQUAL((*fileListMap2)[uuid]->files.empty(), true);
    BOOST_CHECK_EQUAL((*fileListMap2)[uuid]->error, true);
    BOOST_CHECK_EQUAL((*fileListMap2)[uuid]->errorDetails, "details");
    BOOST_CHECK_EQUAL((*fileListMap2)[uuid]->dataReady, true);

    fileListMap2->erase(uuid);
}

BOOST_AUTO_TEST_SUITE_END()

// NOLINTEND(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers,readability-function-cognitive-complexity)