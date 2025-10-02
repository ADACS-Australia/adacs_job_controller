//
// Created by lewis on 12/19/24.
//

import settings;
import job_status;

#include <random>
#include <utility>

#include <boost/lexical_cast.hpp>
#include <boost/test/unit_test.hpp>
#include <boost/uuid/random_generator.hpp>
#include <boost/uuid/uuid_io.hpp>

#include "../../tests/fixtures/DatabaseFixture.h"
#include "../../tests/fixtures/HttpClientFixture.h"
#include "../../tests/fixtures/WebSocketClientFixture.h"

import ClusterManager;
import Cluster;
import FileUpload;
import Message;

struct FileUploadTestDataFixture : public DatabaseFixture, public WebSocketClientFixture, public HttpClientFixture
{
    std::vector<std::vector<uint8_t>> receivedMessages;
    bool bReady = false;
    nlohmann::json jsonClusters;
    std::shared_ptr<FileUpload> fileUpload;
    std::string uuid = boost::lexical_cast<std::string>(boost::uuids::random_generator()());

    FileUploadTestDataFixture()
    {
        // Parse the cluster configuration
        jsonClusters = nlohmann::json::parse(sClusters);

        websocketClient->on_message = [&]([[maybe_unused]] auto connection, auto in_message) {
            onWebsocketMessage(in_message);
        };

        startWebSocketClient();

        // Wait for the client to connect
        while (!bReady)
        {
            std::this_thread::sleep_for(std::chrono::milliseconds(10));
        }

        // Get the last cluster (cluster3) for testing
        auto cluster = clusterManager->getCluster("cluster3");
        fileUpload   = std::static_pointer_cast<FileUpload>(clusterManager->createFileUpload(cluster, uuid));

        fileUpload->stop();
    }

    void onWebsocketMessage(auto in_message)
    {
        auto data = in_message->string();

        // Don't parse the message if the ws connection is ready
        if (!bReady)
        {
            Message msg(std::vector<uint8_t>(data.begin(), data.end()));
            if (msg.getId() == SERVER_READY)
            {
                bReady = true;
                return;
            }
        }

        receivedMessages.emplace_back(std::vector<uint8_t>(data.begin(), data.end()));
    }
};


BOOST_FIXTURE_TEST_SUITE(File_Upload_test_suite, FileUploadTestDataFixture)

BOOST_AUTO_TEST_CASE(test_constructor)
{
    // Check that the cluster details and the manager are set correctly
    auto cluster = clusterManager->getCluster("cluster3");
    BOOST_CHECK_EQUAL(*fileUpload->getpClusterDetails(), cluster->getClusterDetails());

    // Check that the right number of queue levels are created
    BOOST_CHECK_EQUAL(fileUpload->getqueue()->size(), 3);

    // Check that the uuid is correctly set
    BOOST_CHECK_EQUAL(fileUpload->getUuid(), uuid);

    // Check that the file upload object is correctly created
    BOOST_CHECK_EQUAL(fileUpload->fileUploadFileSize, 0);
    BOOST_CHECK_EQUAL(fileUpload->fileUploadError, false);
    BOOST_CHECK_EQUAL(fileUpload->fileUploadErrorDetails.empty(), true);
    BOOST_CHECK_EQUAL(fileUpload->fileUploadDataReady, false);
    BOOST_CHECK_EQUAL(fileUpload->fileUploadReceivedData, false);
    BOOST_CHECK_EQUAL(fileUpload->fileUploadComplete, false);
}

BOOST_AUTO_TEST_CASE(test_handleFileUploadError)
{
    auto msg = Message(FILE_UPLOAD_ERROR);
    msg.push_string("details");  // detail
    fileUpload->handleMessage(msg);

    BOOST_CHECK_EQUAL(fileUpload->fileUploadFileSize, 0);
    BOOST_CHECK_EQUAL(fileUpload->fileUploadError, true);
    BOOST_CHECK_EQUAL(fileUpload->fileUploadErrorDetails, "details");
    BOOST_CHECK_EQUAL(fileUpload->fileUploadDataReady, true);
    BOOST_CHECK_EQUAL(fileUpload->fileUploadReceivedData, false);
    BOOST_CHECK_EQUAL(fileUpload->fileUploadComplete, false);
}

BOOST_AUTO_TEST_CASE(test_handleFileUploadComplete)
{
    auto msg = Message(FILE_UPLOAD_COMPLETE);
    fileUpload->handleMessage(msg);

    BOOST_CHECK_EQUAL(fileUpload->fileUploadFileSize, 0);
    BOOST_CHECK_EQUAL(fileUpload->fileUploadError, false);
    BOOST_CHECK_EQUAL(fileUpload->fileUploadErrorDetails.empty(), true);
    BOOST_CHECK_EQUAL(fileUpload->fileUploadDataReady, true);
    BOOST_CHECK_EQUAL(fileUpload->fileUploadReceivedData, true);
    BOOST_CHECK_EQUAL(fileUpload->fileUploadComplete, true);
}

BOOST_AUTO_TEST_CASE(test_handleServerReady)
{
    // Initially, fileUploadDataReady should be false
    BOOST_CHECK_EQUAL(fileUpload->fileUploadDataReady, false);

    // Send SERVER_READY message
    auto msg = Message(SERVER_READY);
    fileUpload->handleMessage(msg);

    // SERVER_READY should set fileUploadDataReady to true
    BOOST_CHECK_EQUAL(fileUpload->fileUploadDataReady, true);
    BOOST_CHECK_EQUAL(fileUpload->fileUploadError, false);
    BOOST_CHECK_EQUAL(fileUpload->fileUploadComplete, false);
    BOOST_CHECK_EQUAL(fileUpload->fileUploadReceivedData, false);
}

BOOST_AUTO_TEST_CASE(test_invalid_message)
{
    // Test with an invalid message ID
    auto msg = Message(9999);
    fileUpload->handleMessage(msg);

    // State should remain unchanged
    BOOST_CHECK_EQUAL(fileUpload->fileUploadError, false);
    BOOST_CHECK_EQUAL(fileUpload->fileUploadComplete, false);
    BOOST_CHECK_EQUAL(fileUpload->fileUploadDataReady, false);
}


BOOST_AUTO_TEST_SUITE_END()
