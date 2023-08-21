//
// Created by lewis on 2/10/20.
//

#include "../../Lib/JobStatus.h"
#include "../../tests/fixtures/DatabaseFixture.h"
#include "../../tests/fixtures/HttpClientFixture.h"
#include "../../tests/fixtures/WebSocketClientFixture.h"
#include <boost/lexical_cast.hpp>
#include <boost/test/unit_test.hpp>
#include <boost/uuid/random_generator.hpp>
#include <boost/uuid/uuid_io.hpp>
#include <random>
#include <utility>

// NOLINTBEGIN(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers,readability-function-cognitive-complexity)

struct BackgroundThreadsTestDataFixture : public DatabaseFixture, public WebSocketClientFixture {
    std::vector<std::vector<uint8_t>> receivedMessages;
    bool bReady = false;
    nlohmann::json jsonClusters;
    std::shared_ptr<sClusterDetails> onlineDetails;
    std::shared_ptr<Cluster> onlineCluster;
    uint64_t jobId;
    std::string uuid = boost::lexical_cast<std::string>(boost::uuids::random_generator()());

    BackgroundThreadsTestDataFixture() :
        onlineCluster(clusterManager->getvClusters()->front()), onlineDetails(clusterManager->getvClusters()->front()->getClusterDetails())
    {
        // Parse the cluster configuration
        jsonClusters = nlohmann::json::parse(sClusters);

        websocketClient->on_message = [&]([[maybe_unused]] auto connection, auto in_message) {
            onWebsocketMessage(in_message);
        };

        startWebSocketClient();

        // Create a new job object
        jobId = database->run(
                insert_into(jobTable)
                        .set(
                                jobTable.user = 1,
                                jobTable.parameters = "params1",
                                jobTable.cluster = onlineCluster->getName(),
                                jobTable.bundle = "whatever",
                                jobTable.application = "test"
                        )
        );

        // Wait for the client to connect
        while (!bReady) {
            // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
            std::this_thread::sleep_for(std::chrono::milliseconds(10));
        }
    }

    void onWebsocketMessage(auto in_message) {
        auto data = in_message->string();

        // Don't parse the message if the ws connection is ready
        if (!bReady) {
            Message msg(std::vector<uint8_t>(data.begin(), data.end()));
            if (msg.getId() == SERVER_READY) {
                bReady = true;
                return;
            }
        }

        receivedMessages.emplace_back(std::vector<uint8_t>(data.begin(), data.end()));
    };
};


BOOST_FIXTURE_TEST_SUITE(Background_Threads_test_suite, BackgroundThreadsTestDataFixture)
    BOOST_AUTO_TEST_CASE(test_UnsubmittedJobs_pending) {
        // The websocket server and client is already running and connected. Create a job that's marked as unsubmitted
        // and wait a moment, then verify that the job was resubmitted to the client

        // Test PENDING works as expected

        database->run(
                insert_into(jobHistoryTable)
                        .set(
                                jobHistoryTable.jobId = jobId,
                                jobHistoryTable.timestamp = std::chrono::system_clock::now() - std::chrono::seconds{60},
                                jobHistoryTable.what = SYSTEM_SOURCE,
                                jobHistoryTable.state = static_cast<uint32_t>(JobStatus::PENDING),
                                jobHistoryTable.details = "Job pending"
                        )
        );

        for (auto i = 0; i < 5; i++) {
            // Wait until a websocket message is received
            while (receivedMessages.empty()) {
                // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
                std::this_thread::sleep_for(std::chrono::milliseconds(10));
            }

            // The message should be to resubmit the same job
            auto msg = Message(receivedMessages.front());

            receivedMessages.clear();

            BOOST_CHECK_EQUAL(msg.getId(), SUBMIT_JOB);
            BOOST_CHECK_EQUAL(msg.pop_uint(), jobId);
            BOOST_CHECK_EQUAL(msg.pop_string(), "whatever");
            BOOST_CHECK_EQUAL(msg.pop_string(), "params1");
        }
    }

    BOOST_AUTO_TEST_CASE(test_UnsubmittedJobs_submitting) {
        // The websocket server and client is already running and connected. Create a job that's marked as unsubmitted
        // and wait a moment, then verify that the job was resubmitted to the client

        // Test SUBMITTING works as expected

        database->run(
                insert_into(jobHistoryTable)
                        .set(
                                jobHistoryTable.jobId = jobId,
                                jobHistoryTable.timestamp = std::chrono::system_clock::now() - std::chrono::seconds{60},
                                jobHistoryTable.what = SYSTEM_SOURCE,
                                jobHistoryTable.state = static_cast<uint32_t>(JobStatus::SUBMITTING),
                                jobHistoryTable.details = "Job pending"
                        )
        );

        for (auto i = 0; i < 5; i++) {
            // Wait until a websocket message is received
            while (receivedMessages.empty()) {
                // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
                std::this_thread::sleep_for(std::chrono::milliseconds(10));
            }

            // The message should be to resubmit the same job
            auto msg = Message(receivedMessages.front());

            receivedMessages.clear();

            BOOST_CHECK_EQUAL(msg.getId(), SUBMIT_JOB);
            BOOST_CHECK_EQUAL(msg.pop_uint(), jobId);
            BOOST_CHECK_EQUAL(msg.pop_string(), "whatever");
            BOOST_CHECK_EQUAL(msg.pop_string(), "params1");
        }
    }

    BOOST_AUTO_TEST_CASE(test_CancellingJobs) {
        // The websocket server and client is already running and connected. Create a job that's marked as unsubmitted
        // and wait a moment, then verify that the job was resubmitted to the client

        // Test CANCELLING works as expected

        database->run(
                insert_into(jobHistoryTable)
                        .set(
                                jobHistoryTable.jobId = jobId,
                                jobHistoryTable.timestamp = std::chrono::system_clock::now() - std::chrono::seconds{60},
                                jobHistoryTable.what = SYSTEM_SOURCE,
                                jobHistoryTable.state = static_cast<uint32_t>(JobStatus::CANCELLING),
                                jobHistoryTable.details = "Job cancelling"
                        )
        );

        for (auto i = 0; i < 5; i++) {
            // Wait until a websocket message is received
            while (receivedMessages.empty()) {
                // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
                std::this_thread::sleep_for(std::chrono::milliseconds(10));
            }

            // The message should be to resubmit the same job
            auto msg = Message(receivedMessages.front());

            receivedMessages.clear();

            BOOST_CHECK_EQUAL(msg.getId(), CANCEL_JOB);
            BOOST_CHECK_EQUAL(msg.pop_uint(), jobId);
        }
    }

    BOOST_AUTO_TEST_CASE(test_DeletingJobs) {
        // The websocket server and client is already running and connected. Create a job that's marked as unsubmitted
        // and wait a moment, then verify that the job was resubmitted to the client

        // Test DELETING works as expected

        database->run(
                insert_into(jobHistoryTable)
                        .set(
                                jobHistoryTable.jobId = jobId,
                                jobHistoryTable.timestamp = std::chrono::system_clock::now() - std::chrono::seconds{60},
                                jobHistoryTable.what = SYSTEM_SOURCE,
                                jobHistoryTable.state = static_cast<uint32_t>(JobStatus::DELETING),
                                jobHistoryTable.details = "Job cancelling"
                        )
        );

        for (auto i = 0; i < 5; i++) {
            // Wait until a websocket message is received
            while (receivedMessages.empty()) {
                // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
                std::this_thread::sleep_for(std::chrono::milliseconds(10));
            }

            // The message should be to resubmit the same job
            auto msg = Message(receivedMessages.front());

            receivedMessages.clear();

            BOOST_CHECK_EQUAL(msg.getId(), DELETE_JOB);
            BOOST_CHECK_EQUAL(msg.pop_uint(), jobId);
        }
    }

    BOOST_AUTO_TEST_CASE(test_PruneSources) {
        for (auto i = 0; i < 5; i++) {
            // Synchronise the source pruner
            auto tmp_data = generateRandomData(randomInt(0, 255));
            onlineCluster->queueMessage("tmp_data", tmp_data, Message::Priority::Highest);

            while ((*onlineCluster->getqueue())[Message::Priority::Highest].find("tmp_data") !=
                   (*onlineCluster->getqueue())[Message::Priority::Highest].end()) {
                // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
                std::this_thread::sleep_for(std::chrono::milliseconds(1));
            }

            // Create several sources and insert data in the queue
            auto s1_d1 = generateRandomData(randomInt(0, 255));
            onlineCluster->queueMessage("s1", s1_d1, Message::Priority::Highest);

            auto s2_d1 = generateRandomData(randomInt(0, 255));
            onlineCluster->queueMessage("s2", s2_d1, Message::Priority::Lowest);

            auto s3_d1 = generateRandomData(randomInt(0, 255));
            onlineCluster->queueMessage("s3", s3_d1, Message::Priority::Lowest);

            // All sources should exist at this point in the queue because not enough time has elapsed to run the
            // source pruner
            BOOST_CHECK_EQUAL((*onlineCluster->getqueue())[Message::Priority::Highest].find("s1") ==
                              (*onlineCluster->getqueue())[Message::Priority::Highest].end(), false);
            BOOST_CHECK_EQUAL((*onlineCluster->getqueue())[Message::Priority::Lowest].find("s2") ==
                              (*onlineCluster->getqueue())[Message::Priority::Lowest].end(), false);
            BOOST_CHECK_EQUAL((*onlineCluster->getqueue())[Message::Priority::Lowest].find("s3") ==
                              (*onlineCluster->getqueue())[Message::Priority::Lowest].end(), false);

            // Wait for the source pruner to run ( * 2 here to handle processing times in the test )
            std::this_thread::sleep_for(std::chrono::milliseconds(QUEUE_SOURCE_PRUNE_MILLISECONDS * 2));

            // All messages should have been sent on the websocket now, leaving empty queues for each source. The
            // pruner should have cleaned up each source
            BOOST_CHECK_EQUAL((*onlineCluster->getqueue())[Message::Priority::Highest].find("s1") ==
                              (*onlineCluster->getqueue())[Message::Priority::Highest].end(), true);
            BOOST_CHECK_EQUAL((*onlineCluster->getqueue())[Message::Priority::Lowest].find("s2") ==
                              (*onlineCluster->getqueue())[Message::Priority::Lowest].end(), true);
            BOOST_CHECK_EQUAL((*onlineCluster->getqueue())[Message::Priority::Lowest].find("s3") ==
                              (*onlineCluster->getqueue())[Message::Priority::Lowest].end(), true);
        }

        // There should be 5 x (3 + 1) messages sent over the websocket total
        BOOST_CHECK_EQUAL(receivedMessages.size(), 5 * 4);
    }
BOOST_AUTO_TEST_SUITE_END()

// NOLINTEND(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers,readability-function-cognitive-complexity)