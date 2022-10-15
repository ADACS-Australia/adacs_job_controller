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

struct FileDownloadTestDataFixture : public DatabaseFixture, public WebSocketClientFixture, public HttpClientFixture {
    std::vector<std::vector<uint8_t>> receivedMessages;
    bool bReady = false;
    nlohmann::json jsonClusters;
    std::shared_ptr<FileDownload> fileDownload;
    std::string uuid = boost::lexical_cast<std::string>(boost::uuids::random_generator()());

    FileDownloadTestDataFixture() {
        // Parse the cluster configuration
        jsonClusters = nlohmann::json::parse(sClusters);

        websocketClient->on_message = [&]([[maybe_unused]] auto connection, auto in_message) {
            onWebsocketMessage(in_message);
        };

        startWebSocketClient();

        // Wait for the client to connect
        while (!bReady) {
            // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
            std::this_thread::sleep_for(std::chrono::milliseconds(10));
        }

        fileDownload = clusterManager->createFileDownload(clusterManager->getvClusters()->back(), uuid);
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


BOOST_FIXTURE_TEST_SUITE(File_Download_test_suite, FileDownloadTestDataFixture)
    BOOST_AUTO_TEST_CASE(test_constructor) {
        // Check that the cluster details and the manager are set correctly
        BOOST_CHECK_EQUAL(*fileDownload->getpClusterDetails(), clusterManager->getvClusters()->back()->getClusterDetails());

        // Check that the right number of queue levels are created (+1 because 0 is a priority level itself)
        BOOST_CHECK_EQUAL(fileDownload->getqueue()->size(),
                          static_cast<uint32_t>(Message::Priority::Lowest) - static_cast<uint32_t>(Message::Priority::Highest) + 1);

        // Check that the uuid is correctly set
        BOOST_CHECK_EQUAL(fileDownload->getUuid(), uuid);

        // Check that the file download object is correctly created
        BOOST_CHECK_EQUAL(fileDownload->fileDownloadFileSize, -1);
        BOOST_CHECK_EQUAL(fileDownload->fileDownloadError, false);
        BOOST_CHECK_EQUAL(fileDownload->fileDownloadErrorDetails.empty(), true);
        BOOST_CHECK_EQUAL(fileDownload->fileDownloadDataReady, false);
        BOOST_CHECK_EQUAL(fileDownload->fileDownloadReceivedData, false);
        BOOST_CHECK_EQUAL(fileDownload->fileDownloadReceivedBytes, 0);
        BOOST_CHECK_EQUAL(fileDownload->fileDownloadSentBytes, 0);
        BOOST_CHECK_EQUAL(fileDownload->fileDownloadClientPaused, false);
    }

    BOOST_AUTO_TEST_CASE(test_handleFileChunk) {
        auto chunk = generateRandomData(randomInt(0, 255));

        auto msg = Message(FILE_CHUNK);
        msg.push_bytes(*chunk);     // chunk
        fileDownload->handleMessage(msg);

        BOOST_CHECK_EQUAL(fileDownload->fileDownloadFileSize, -1);
        BOOST_CHECK_EQUAL(fileDownload->fileDownloadError, false);
        BOOST_CHECK_EQUAL(fileDownload->fileDownloadErrorDetails.empty(), true);
        BOOST_CHECK_EQUAL(fileDownload->fileDownloadDataReady, true);
        BOOST_CHECK_EQUAL(fileDownload->fileDownloadReceivedData, false);
        BOOST_CHECK_EQUAL(fileDownload->fileDownloadReceivedBytes, chunk->size());
        BOOST_CHECK_EQUAL(fileDownload->fileDownloadSentBytes, 0);
        BOOST_CHECK_EQUAL(fileDownload->fileDownloadClientPaused, false);

        BOOST_CHECK_EQUAL_COLLECTIONS(chunk->begin(), chunk->end(), (*fileDownload->fileDownloadQueue.try_peek())->begin(),
                                      (*fileDownload->fileDownloadQueue.try_peek())->end());

        std::vector<std::shared_ptr<std::vector<uint8_t>>> chunks = {chunk};

        // Fill the queue and make sure that a pause file chunk stream isn't sent until the queue is full
        while (!fileDownload->fileDownloadClientPaused) {
            chunk = generateRandomData(randomInt(0, 255));
            chunks.push_back(chunk);

            if (!(*fileDownload->getqueue())[Message::Priority::Highest].empty()) {
                BOOST_ASSERT("PAUSE_FILE_CHUNK_STREAM was sent before it should have been");
            }

            msg = Message(FILE_CHUNK);
            msg.push_bytes(*chunk);     // chunk
            fileDownload->handleMessage(msg);
        }

        // Check that a pause file chunk stream message was sent
        BOOST_CHECK_EQUAL((*fileDownload->getqueue())[Message::Priority::Highest].size(), 1);
        auto ptr = *(*fileDownload->getqueue())[Message::Priority::Highest].find(uuid)->second->try_dequeue();
        msg = Message(*ptr);
        BOOST_CHECK_EQUAL(msg.getId(), PAUSE_FILE_CHUNK_STREAM);
        BOOST_CHECK_EQUAL(msg.pop_string(), uuid);

        // Verify that the chunks were correctly queued
        bool different = false;
        for (const auto& chunk : chunks) {
            auto queueChunk = (*fileDownload->fileDownloadQueue.try_dequeue());
            different = different || !std::equal((*queueChunk).begin(), (*queueChunk).end(), (*chunk).begin());
        }

        BOOST_CHECK_EQUAL(different, false);
    }

    BOOST_AUTO_TEST_CASE(test_handleFileError) {
        auto msg = Message(FILE_ERROR);
        msg.push_string("details");     // detail
        fileDownload->handleMessage(msg);

        BOOST_CHECK_EQUAL(fileDownload->fileDownloadFileSize, -1);
        BOOST_CHECK_EQUAL(fileDownload->fileDownloadError, true);
        BOOST_CHECK_EQUAL(fileDownload->fileDownloadErrorDetails, "details");
        BOOST_CHECK_EQUAL(fileDownload->fileDownloadDataReady, true);
        BOOST_CHECK_EQUAL(fileDownload->fileDownloadReceivedData, false);
        BOOST_CHECK_EQUAL(fileDownload->fileDownloadReceivedBytes, 0);
        BOOST_CHECK_EQUAL(fileDownload->fileDownloadSentBytes, 0);
        BOOST_CHECK_EQUAL(fileDownload->fileDownloadClientPaused, false);
    }

    BOOST_AUTO_TEST_CASE(test_handleFileDetails) {
        auto msg = Message(FILE_DETAILS);
        msg.push_ulong(0x123456789abcdef);      // fileSize
        fileDownload->handleMessage(msg);

        BOOST_CHECK_EQUAL(fileDownload->fileDownloadFileSize, 0x123456789abcdef);
        BOOST_CHECK_EQUAL(fileDownload->fileDownloadError, false);
        BOOST_CHECK_EQUAL(fileDownload->fileDownloadErrorDetails.empty(), true);
        BOOST_CHECK_EQUAL(fileDownload->fileDownloadDataReady, true);
        BOOST_CHECK_EQUAL(fileDownload->fileDownloadReceivedData, true);
        BOOST_CHECK_EQUAL(fileDownload->fileDownloadReceivedBytes, 0);
        BOOST_CHECK_EQUAL(fileDownload->fileDownloadSentBytes, 0);
        BOOST_CHECK_EQUAL(fileDownload->fileDownloadClientPaused, false);
    }
BOOST_AUTO_TEST_SUITE_END()

// NOLINTEND(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers,readability-function-cognitive-complexity)