//
// Created by lewis on 12/19/24.
//

import settings;
import job_status;

#include "../../tests/fixtures/DatabaseFixture.h"
#include "../../tests/fixtures/HttpClientFixture.h"
#include "../../tests/fixtures/WebSocketClientFixture.h"
#include <boost/lexical_cast.hpp>
#include <boost/test/unit_test.hpp>
#include <boost/uuid/random_generator.hpp>
#include <boost/uuid/uuid_io.hpp>
#include <random>
#include <utility>

import ClusterManager;
import Cluster;
import FileUpload;
import Message;
// NOLINTBEGIN(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers,readability-function-cognitive-complexity)

struct FileUploadTestDataFixture : public DatabaseFixture, public WebSocketClientFixture, public HttpClientFixture {
    std::vector<std::vector<uint8_t>> receivedMessages;
    bool bReady = false;
    nlohmann::json jsonClusters;
    std::shared_ptr<FileUpload> fileUpload;
    std::string uuid = boost::lexical_cast<std::string>(boost::uuids::random_generator()());

    FileUploadTestDataFixture() {
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

        // Get the last cluster (cluster3) for testing
        auto cluster = clusterManager->getCluster("cluster3");
        fileUpload = std::static_pointer_cast<FileUpload>(clusterManager->createFileUpload(cluster, uuid));

        fileUpload->stop();
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


BOOST_FIXTURE_TEST_SUITE(File_Upload_test_suite, FileUploadTestDataFixture)
    BOOST_AUTO_TEST_CASE(test_constructor) {
        // Check that the cluster details and the manager are set correctly
        auto cluster = clusterManager->getCluster("cluster3");
        BOOST_CHECK_EQUAL(*fileUpload->getpClusterDetails(), cluster->getClusterDetails());

        // Check that the right number of queue levels are created (+1 because 0 is a priority level itself)
        BOOST_CHECK_EQUAL(fileUpload->getqueue()->size(),
                          static_cast<uint32_t>(Message::Priority::Lowest) - static_cast<uint32_t>(Message::Priority::Highest) + 1);

        // Check that the uuid is correctly set
        BOOST_CHECK_EQUAL(fileUpload->getUuid(), uuid);

        // Check that the file upload object is correctly created
        BOOST_CHECK_EQUAL(fileUpload->fileUploadFileSize, 0);
        BOOST_CHECK_EQUAL(fileUpload->fileUploadError, false);
        BOOST_CHECK_EQUAL(fileUpload->fileUploadErrorDetails.empty(), true);
        BOOST_CHECK_EQUAL(fileUpload->fileUploadDataReady, false);
        BOOST_CHECK_EQUAL(fileUpload->fileUploadReceivedData, false);
        BOOST_CHECK_EQUAL(fileUpload->fileUploadSentBytes, 0);
        BOOST_CHECK_EQUAL(fileUpload->fileUploadComplete, false);
    }

    BOOST_AUTO_TEST_CASE(test_handleFileUploadChunk) {
        auto chunk = generateRandomData(randomInt(0, 255));

        auto msg = Message(FILE_UPLOAD_CHUNK);
        msg.push_bytes(*chunk);     // chunk
        fileUpload->handleMessage(msg);

        BOOST_CHECK_EQUAL(fileUpload->fileUploadFileSize, 0);
        BOOST_CHECK_EQUAL(fileUpload->fileUploadError, false);
        BOOST_CHECK_EQUAL(fileUpload->fileUploadErrorDetails.empty(), true);
        BOOST_CHECK_EQUAL(fileUpload->fileUploadDataReady, true);
        BOOST_CHECK_EQUAL(fileUpload->fileUploadReceivedData, false);
        BOOST_CHECK_EQUAL(fileUpload->fileUploadSentBytes, chunk->size());
        BOOST_CHECK_EQUAL(fileUpload->fileUploadComplete, false);
    }

    BOOST_AUTO_TEST_CASE(test_handleFileUploadError) {
        auto msg = Message(FILE_UPLOAD_ERROR);
        msg.push_string("details");     // detail
        fileUpload->handleMessage(msg);

        BOOST_CHECK_EQUAL(fileUpload->fileUploadFileSize, 0);
        BOOST_CHECK_EQUAL(fileUpload->fileUploadError, true);
        BOOST_CHECK_EQUAL(fileUpload->fileUploadErrorDetails, "details");
        BOOST_CHECK_EQUAL(fileUpload->fileUploadDataReady, true);
        BOOST_CHECK_EQUAL(fileUpload->fileUploadReceivedData, false);
        BOOST_CHECK_EQUAL(fileUpload->fileUploadSentBytes, 0);
        BOOST_CHECK_EQUAL(fileUpload->fileUploadComplete, false);
    }

    BOOST_AUTO_TEST_CASE(test_handleFileUploadComplete) {
        auto msg = Message(FILE_UPLOAD_COMPLETE);
        fileUpload->handleMessage(msg);

        BOOST_CHECK_EQUAL(fileUpload->fileUploadFileSize, 0);
        BOOST_CHECK_EQUAL(fileUpload->fileUploadError, false);
        BOOST_CHECK_EQUAL(fileUpload->fileUploadErrorDetails.empty(), true);
        BOOST_CHECK_EQUAL(fileUpload->fileUploadDataReady, true);
        BOOST_CHECK_EQUAL(fileUpload->fileUploadReceivedData, true);
        BOOST_CHECK_EQUAL(fileUpload->fileUploadSentBytes, 0);
        BOOST_CHECK_EQUAL(fileUpload->fileUploadComplete, true);
    }

    BOOST_AUTO_TEST_CASE(test_multiple_chunks) {
        std::vector<std::shared_ptr<std::vector<uint8_t>>> chunks;
        uint64_t totalBytes = 0;

        // Send multiple chunks
        for (int i = 0; i < 10; i++) {
            auto chunk = generateRandomData(randomInt(1, 100));
            chunks.push_back(chunk);
            totalBytes += chunk->size();

            auto msg = Message(FILE_UPLOAD_CHUNK);
            msg.push_bytes(*chunk);
            fileUpload->handleMessage(msg);
        }

        BOOST_CHECK_EQUAL(fileUpload->fileUploadSentBytes, totalBytes);
        BOOST_CHECK_EQUAL(fileUpload->fileUploadDataReady, true);
        BOOST_CHECK_EQUAL(fileUpload->fileUploadError, false);
        BOOST_CHECK_EQUAL(fileUpload->fileUploadComplete, false);
    }

    BOOST_AUTO_TEST_CASE(test_error_after_chunks) {
        // Send some chunks first
        auto chunk = generateRandomData(100);
        auto msg = Message(FILE_UPLOAD_CHUNK);
        msg.push_bytes(*chunk);
        fileUpload->handleMessage(msg);

        BOOST_CHECK_EQUAL(fileUpload->fileUploadSentBytes, 100);
        BOOST_CHECK_EQUAL(fileUpload->fileUploadError, false);

        // Then send an error
        msg = Message(FILE_UPLOAD_ERROR);
        msg.push_string("Upload failed");
        fileUpload->handleMessage(msg);

        BOOST_CHECK_EQUAL(fileUpload->fileUploadError, true);
        BOOST_CHECK_EQUAL(fileUpload->fileUploadErrorDetails, "Upload failed");
        BOOST_CHECK_EQUAL(fileUpload->fileUploadDataReady, true);
        // Sent bytes should remain unchanged
        BOOST_CHECK_EQUAL(fileUpload->fileUploadSentBytes, 100);
    }

    BOOST_AUTO_TEST_CASE(test_complete_after_chunks) {
        // Send some chunks first
        auto chunk = generateRandomData(100);
        auto msg = Message(FILE_UPLOAD_CHUNK);
        msg.push_bytes(*chunk);
        fileUpload->handleMessage(msg);

        BOOST_CHECK_EQUAL(fileUpload->fileUploadSentBytes, 100);
        BOOST_CHECK_EQUAL(fileUpload->fileUploadComplete, false);

        // Then send completion
        msg = Message(FILE_UPLOAD_COMPLETE);
        fileUpload->handleMessage(msg);

        BOOST_CHECK_EQUAL(fileUpload->fileUploadComplete, true);
        BOOST_CHECK_EQUAL(fileUpload->fileUploadReceivedData, true);
        BOOST_CHECK_EQUAL(fileUpload->fileUploadDataReady, true);
        // Sent bytes should remain unchanged
        BOOST_CHECK_EQUAL(fileUpload->fileUploadSentBytes, 100);
    }

    BOOST_AUTO_TEST_CASE(test_invalid_message) {
        // Test with an invalid message ID
        auto msg = Message(9999);
        fileUpload->handleMessage(msg);

        // State should remain unchanged
        BOOST_CHECK_EQUAL(fileUpload->fileUploadSentBytes, 0);
        BOOST_CHECK_EQUAL(fileUpload->fileUploadError, false);
        BOOST_CHECK_EQUAL(fileUpload->fileUploadComplete, false);
        BOOST_CHECK_EQUAL(fileUpload->fileUploadDataReady, false);
    }

    BOOST_AUTO_TEST_CASE(test_condition_variable_signaling) {
        std::atomic<bool> dataReady{false};
        
        // Start a thread that waits for data ready
        std::thread waiter([&]() {
            std::unique_lock<std::mutex> lock(fileUpload->fileUploadDataCVMutex);
            dataReady = fileUpload->fileUploadDataCV.wait_for(lock, std::chrono::milliseconds(100), 
                [&] { return fileUpload->fileUploadDataReady; });
        });

        // Send a chunk to trigger the condition variable
        auto chunk = generateRandomData(10);
        auto msg = Message(FILE_UPLOAD_CHUNK);
        msg.push_bytes(*chunk);
        
        fileUpload->handleMessage(msg);

        waiter.join();
        BOOST_CHECK(dataReady);
    }

    BOOST_AUTO_TEST_CASE(test_large_chunk_handling) {
        // Test with a large chunk
        auto largeChunk = generateRandomData(1024 * 1024); // 1MB chunk
        
        auto msg = Message(FILE_UPLOAD_CHUNK);
        msg.push_bytes(*largeChunk);
        fileUpload->handleMessage(msg);

        BOOST_CHECK_EQUAL(fileUpload->fileUploadSentBytes, largeChunk->size());
        BOOST_CHECK_EQUAL(fileUpload->fileUploadDataReady, true);
        BOOST_CHECK_EQUAL(fileUpload->fileUploadError, false);
    }

    BOOST_AUTO_TEST_CASE(test_empty_chunk) {
        // Test with an empty chunk
        auto emptyChunk = std::make_shared<std::vector<uint8_t>>();
        
        auto msg = Message(FILE_UPLOAD_CHUNK);
        msg.push_bytes(*emptyChunk);
        fileUpload->handleMessage(msg);

        BOOST_CHECK_EQUAL(fileUpload->fileUploadSentBytes, 0);
        BOOST_CHECK_EQUAL(fileUpload->fileUploadDataReady, true);
        BOOST_CHECK_EQUAL(fileUpload->fileUploadError, false);
    }

    BOOST_AUTO_TEST_CASE(test_state_transitions) {
        // Test various state transitions
        
        // Initial state
        BOOST_CHECK_EQUAL(fileUpload->fileUploadError, false);
        BOOST_CHECK_EQUAL(fileUpload->fileUploadComplete, false);
        BOOST_CHECK_EQUAL(fileUpload->fileUploadReceivedData, false);

        // Send chunks
        auto chunk = generateRandomData(50);
        auto msg = Message(FILE_UPLOAD_CHUNK);
        msg.push_bytes(*chunk);
        fileUpload->handleMessage(msg);

        BOOST_CHECK_EQUAL(fileUpload->fileUploadSentBytes, 50);
        BOOST_CHECK_EQUAL(fileUpload->fileUploadError, false);
        BOOST_CHECK_EQUAL(fileUpload->fileUploadComplete, false);

        // Send completion
        msg = Message(FILE_UPLOAD_COMPLETE);
        fileUpload->handleMessage(msg);

        BOOST_CHECK_EQUAL(fileUpload->fileUploadComplete, true);
        BOOST_CHECK_EQUAL(fileUpload->fileUploadReceivedData, true);
        BOOST_CHECK_EQUAL(fileUpload->fileUploadError, false);
    }

    BOOST_AUTO_TEST_CASE(test_error_handling_priority) {
        // Test that error handling takes priority over other operations
        
        // Send some chunks
        auto chunk = generateRandomData(100);
        auto msg = Message(FILE_UPLOAD_CHUNK);
        msg.push_bytes(*chunk);
        fileUpload->handleMessage(msg);

        BOOST_CHECK_EQUAL(fileUpload->fileUploadSentBytes, 100);
        BOOST_CHECK_EQUAL(fileUpload->fileUploadError, false);

        // Send error - should override previous state
        msg = Message(FILE_UPLOAD_ERROR);
        msg.push_string("Critical error");
        fileUpload->handleMessage(msg);

        BOOST_CHECK_EQUAL(fileUpload->fileUploadError, true);
        BOOST_CHECK_EQUAL(fileUpload->fileUploadErrorDetails, "Critical error");
        BOOST_CHECK_EQUAL(fileUpload->fileUploadDataReady, true);
    }

    BOOST_AUTO_TEST_CASE(test_completion_handling_priority) {
        // Test that completion handling works correctly
        
        // Send some chunks
        auto chunk = generateRandomData(200);
        auto msg = Message(FILE_UPLOAD_CHUNK);
        msg.push_bytes(*chunk);
        fileUpload->handleMessage(msg);

        BOOST_CHECK_EQUAL(fileUpload->fileUploadSentBytes, 200);
        BOOST_CHECK_EQUAL(fileUpload->fileUploadComplete, false);

        // Send completion - should finalize the upload
        msg = Message(FILE_UPLOAD_COMPLETE);
        fileUpload->handleMessage(msg);

        BOOST_CHECK_EQUAL(fileUpload->fileUploadComplete, true);
        BOOST_CHECK_EQUAL(fileUpload->fileUploadReceivedData, true);
        BOOST_CHECK_EQUAL(fileUpload->fileUploadDataReady, true);
        BOOST_CHECK_EQUAL(fileUpload->fileUploadError, false);
    }
BOOST_AUTO_TEST_SUITE_END()

// NOLINTEND(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers,readability-function-cognitive-complexity)
