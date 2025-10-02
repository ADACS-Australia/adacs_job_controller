//
// Created by lewis on 12/19/24.
//

import settings;
#include "../../tests/fixtures/DatabaseFixture.h"
#include "../../tests/fixtures/HttpClientFixture.h"
#include "../../tests/fixtures/WebSocketClientFixture.h"
#include "utils.h"
#include <boost/test/unit_test.hpp>
#include <cstddef>

import ClusterManager;
import Cluster;
import ICluster;
import FileUpload;
import Message;


struct FileUploadTransferTestDataFixture : public DatabaseFixture, public WebSocketClientFixture, public HttpClientFixture
{
    std::shared_ptr<ICluster> cluster;
    uint64_t jobId;
    std::promise<void> readyPromise;
    std::function<void(const Message&, const std::shared_ptr<TestWsClient::Connection>&)> fileUploadCallback;
    std::shared_ptr<TestWsClient> websocketFileUploadClient;
    std::shared_ptr<std::jthread> fileUploadThread;
    std::vector<uint8_t> fileData;
    bool bPaused = false;
    std::jthread sendDataThread;
    
    FileUploadTransferTestDataFixture() :
            cluster(clusterManager->getCluster("cluster1"))
    {
        jobId = database->run(
                insert_into(jobTable)
                        .set(
                                jobTable.user = 1,
                                jobTable.parameters = "params1",
                                jobTable.cluster = cluster->getName(),
                                jobTable.bundle = "whatever",
                                jobTable.application = std::static_pointer_cast<HttpServer>(httpServer)->getvJwtSecrets()->back().name()
                        )
        );

        websocketClient->on_message = [this](const std::shared_ptr<TestWsClient::Connection>& connection, const std::shared_ptr<TestWsClient::InMessage>& in_message) {
            onWebsocketMessage(connection, in_message);
        };
    }

    ~FileUploadTransferTestDataFixture() {
        if (websocketFileUploadClient) {
            websocketFileUploadClient->stop();
        }
    }

    FileUploadTransferTestDataFixture(FileUploadTransferTestDataFixture const&) = delete;
    auto operator =(FileUploadTransferTestDataFixture const&) -> FileUploadTransferTestDataFixture& = delete;
    FileUploadTransferTestDataFixture(FileUploadTransferTestDataFixture&&) = delete;
    auto operator=(FileUploadTransferTestDataFixture&&) -> FileUploadTransferTestDataFixture& = delete;

    void onWebsocketMessage(const std::shared_ptr<TestWsClient::Connection>& /*connection*/, const std::shared_ptr<TestWsClient::InMessage>& in_message) {
        auto data = in_message->string();
        auto msg = Message(std::vector<uint8_t>(data.begin(), data.end()));

        // Ignore the ready message
        if (msg.getId() == SERVER_READY) {
            readyPromise.set_value();
            return;
        }

        if (msg.getId() == UPLOAD_FILE) {
            msg.pop_string(); // targetPath
            auto fileSize = msg.pop_ulong();

            websocketFileUploadClient = std::make_shared<TestWsClient>("localhost:8001/job/ws/?token=" + std::to_string(fileSize));
            websocketFileUploadClient->on_message = [this](
                    const std::shared_ptr<TestWsClient::Connection>& connection,
                    const std::shared_ptr<TestWsClient::InMessage>& in_message) {
                auto data = in_message->string();
                auto msg = Message(std::vector<uint8_t>(data.begin(), data.end()));
                fileUploadCallback(msg, connection);
            };

            // Start the client
            fileUploadThread = std::make_shared<std::jthread>([this]() {
                websocketFileUploadClient->start();
            });

            return;
        }

        BOOST_FAIL("Master client got unexpected message id " + std::to_string(msg.getId()));
    }

    auto requestFileUploadId() -> std::string {
        // Create a file upload
        setJwtSecret(std::static_pointer_cast<HttpServer>(httpServer)->getvJwtSecrets()->back().secret());

        // Create params
        nlohmann::json params = {
                {"jobId", jobId},
                {"targetPath",  "/data/myfile.png"}
        };

        auto response = httpClient.request("PUT", "/job/apiv1/file/upload/", params.dump(), {{"Authorization", jwtToken.signature()}});

        nlohmann::json result;
        response->content >> result;

        return {result["uploadId"]};
    }

    static void sendMessage(Message* msg, const std::shared_ptr<TestWsClient::Connection>& connection) {
        auto message = std::make_shared<TestWsClient::OutMessage>(msg->getdata()->get()->size());
        std::ostream_iterator<uint8_t> iter(*message);
        std::copy(msg->getdata()->get()->begin(), msg->getdata()->get()->end(), iter);
        // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
        connection->send(message, nullptr, 130);
    }

    auto generateRandomData(size_t size) -> std::shared_ptr<std::vector<uint8_t>> {
        auto data = std::make_shared<std::vector<uint8_t>>(size);
        for (size_t i = 0; i < size; i++) {
            (*data)[i] = static_cast<uint8_t>(rand() % 256); // NOLINT(cppcoreguidelines-avoid-magic-numbers,concurrency-mt-unsafe)
        }
        return data;
    }

    auto randomInt(uint64_t min, uint64_t max) -> uint64_t {
        return min + (rand() % (max - min + 1)); // NOLINT(cppcoreguidelines-avoid-magic-numbers,concurrency-mt-unsafe)
    }
};

BOOST_FIXTURE_TEST_SUITE(file_upload_transfer_test_suite, FileUploadTransferTestDataFixture)
    BOOST_AUTO_TEST_CASE(test_file_upload_transfer) {
        fileUploadCallback = [&](const Message& msg, const std::shared_ptr<TestWsClient::Connection>& connection) {
            // Use the ready message as our prompt to start sending file data
            if (msg.getId() != SERVER_READY) {
                BOOST_FAIL("File Upload client got unexpected message id " + std::to_string(msg.getId()));
                return;
            }

            // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
            auto fileSize = this->randomInt(0, 1024ULL*1024ULL);
            fileData.reserve(fileSize);

            // Send the file size to the server
            auto newmsg = Message(FILE_UPLOAD_DETAILS, Message::Priority::Highest, "");
            newmsg.push_ulong(fileSize);

            sendMessage(&newmsg, connection);

            // Now send the file content in to chunks and send it to the client
            sendDataThread = std::jthread([this, connection, fileSize]() {
                auto CHUNK_SIZE = FILE_CHUNK_SIZE;

                uint64_t bytesSent = 0;
                while (bytesSent < fileSize) {
                    // Don't do anything while the stream is paused
                    while (bPaused) {
                        std::this_thread::yield();
                    }

                    auto chunkSize = std::min(static_cast<uint32_t>(CHUNK_SIZE), static_cast<uint32_t>(fileSize-bytesSent));
                    bytesSent += chunkSize;

                    auto data = generateRandomData(chunkSize);

                    fileData.insert(fileData.end(), (*data).begin(), (*data).end());

                    auto msg = Message(FILE_UPLOAD_CHUNK, Message::Priority::Lowest, "");
                    msg.push_bytes(*data);

                    sendMessage(&msg, connection);
                }

                // Send completion message
                auto completeMsg = Message(FILE_UPLOAD_COMPLETE, Message::Priority::Highest, "");
                sendMessage(&completeMsg, connection);
            });
        };

        this->startWebSocketClient();
        readyPromise.get_future().wait();

        // Create a file upload ID
        auto uploadId = this->requestFileUploadId();

        // Try to upload the file
        // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
        for (int i = 0; i < 5; i++) {
            // Try to upload the file
            auto response = httpClient.request("PUT", "/job/apiv1/file/upload/", 
                nlohmann::json{{"jobId", jobId}, {"targetPath", "/data/myfile.png"}}.dump(),
                {{"Authorization", jwtToken.signature()}, {"Content-Length", std::to_string(fileData.size())}});

            // Check that the upload was successful
            BOOST_CHECK_EQUAL(response->status_code, "200");

            auto result = nlohmann::json::parse(response->content.string());
            BOOST_CHECK(result.contains("uploadId"));
            BOOST_CHECK_EQUAL(result["status"], "completed");

            fileData.clear();

            sendDataThread.join();
            websocketFileUploadClient->stop();
            fileUploadThread->join();
        }
    }

    BOOST_AUTO_TEST_CASE(test_large_file_uploads) {
        fileUploadCallback = [&](const Message& msg, const std::shared_ptr<TestWsClient::Connection>& connection) {
            // Use the ready message as our prompt to start sending file data
            if (msg.getId() != SERVER_READY) {
                BOOST_FAIL("File Upload client got unexpected message id " + std::to_string(msg.getId()));
                return;
            }

            // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
            auto fileSize = this->randomInt(1024ULL*1024ULL, 10ULL*1024ULL*1024ULL);
            fileData.reserve(fileSize);

            // Send the file size to the server
            auto newmsg = Message(FILE_UPLOAD_DETAILS, Message::Priority::Highest, "");
            newmsg.push_ulong(fileSize);

            sendMessage(&newmsg, connection);

            // Now send the file content in to chunks and send it to the client
            sendDataThread = std::jthread([this, connection, fileSize]() {
                auto CHUNK_SIZE = FILE_CHUNK_SIZE;

                auto data = std::vector<uint8_t>();

                uint64_t bytesSent = 0;
                while (bytesSent < fileSize) {
                    // Spin while the stream is paused
                    while (bPaused) {
                        std::this_thread::yield();
                    }

                    auto chunkSize = std::min(static_cast<uint64_t>(CHUNK_SIZE),
                                              static_cast<uint64_t>(fileSize - bytesSent));
                    bytesSent += chunkSize;
                    data.resize(chunkSize);

                    auto msg = Message(FILE_UPLOAD_CHUNK, Message::Priority::Lowest, "");
                    msg.push_bytes(data);

                    auto smsg = Message(**msg.getdata());
                    std::static_pointer_cast<ClusterManager>(clusterManager)->getmConnectedFileUploads()->begin()->second->callhandleMessage(smsg);
                }

                // Send completion message
                auto completeMsg = Message(FILE_UPLOAD_COMPLETE, Message::Priority::Highest, "");
                auto smsg = Message(**completeMsg.getdata());
                std::static_pointer_cast<ClusterManager>(clusterManager)->getmConnectedFileUploads()->begin()->second->callhandleMessage(smsg);
            });
        };

        this->startWebSocketClient();
        readyPromise.get_future().wait();

        // Create a file upload ID
        auto uploadId = this->requestFileUploadId();

        // Try to upload the file
        auto baselineMemUsage = static_cast<int64_t>(getCurrentMemoryUsage());
        uint64_t totalBytesReceived = 0;
        bool end = false;
        // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
        httpClient.config.max_response_streambuf_size = static_cast<std::size_t>(1024*1024);
        auto response = httpClient.request("PUT", "/job/apiv1/file/upload/", 
            nlohmann::json{{"jobId", jobId}, {"targetPath", "/data/myfile.png"}}.dump(),
            {{"Authorization", jwtToken.signature()}, {"Content-Length", std::to_string(fileData.size())}});

        // Check that the upload was successful
        BOOST_CHECK_EQUAL(std::stoi(response->status_code), 200);

        auto result = nlohmann::json::parse(response->content.string());
        BOOST_CHECK(result.contains("uploadId"));
        BOOST_CHECK_EQUAL(result["status"], "completed");

        // Check that memory usage didn't grow too much
        auto finalMemUsage = static_cast<int64_t>(getCurrentMemoryUsage());
        auto memGrowth = finalMemUsage - baselineMemUsage;
        // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
        BOOST_CHECK_LT(memGrowth, 50 * 1024 * 1024); // Should not grow by more than 50MB

        sendDataThread.join();
        websocketFileUploadClient->stop();
        fileUploadThread->join();
    }

    BOOST_AUTO_TEST_CASE(test_continuous_file_uploads) {
        fileUploadCallback = [&](const Message& msg, const std::shared_ptr<TestWsClient::Connection>& connection) {
            // Use the ready message as our prompt to start sending file data
            if (msg.getId() != SERVER_READY) {
                BOOST_FAIL("File Upload client got unexpected message id " + std::to_string(msg.getId()));
                return;
            }

            // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
            auto fileSize = this->randomInt(1024ULL*1024ULL, 5ULL*1024ULL*1024ULL);
            fileData.reserve(fileSize);

            // Send the file size to the server
            auto newmsg = Message(FILE_UPLOAD_DETAILS, Message::Priority::Highest, "");
            newmsg.push_ulong(fileSize);

            sendMessage(&newmsg, connection);

            // Now send the file content in to chunks and send it to the client
            bool bRunning = true;
            sendDataThread = std::jthread([this, connection, &bRunning]() {
                auto CHUNK_SIZE = FILE_CHUNK_SIZE;

                auto data = std::vector<uint8_t>();
                data.resize(CHUNK_SIZE);

                while (bRunning) {
                    auto msg = Message(FILE_UPLOAD_CHUNK, Message::Priority::Lowest, "");
                    msg.push_bytes(data);

                    auto smsg = Message(**msg.getdata());
                    auto concreteManager = std::static_pointer_cast<ClusterManager>(clusterManager);
                    if (!concreteManager->getmConnectedFileUploads()->empty() && concreteManager->getmConnectedFileUploads()->begin()->second != nullptr) {
                        concreteManager->getmConnectedFileUploads()->begin()->second->callhandleMessage(smsg);
                    }
                }
            });
        };

        this->startWebSocketClient();
        readyPromise.get_future().wait();

        // Create a file upload ID
        auto uploadId = this->requestFileUploadId();

        // Try to upload the file
        uint64_t totalBytesReceived = 0;
        bool end = false;
        // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
        httpClient.config.max_response_streambuf_size = static_cast<std::size_t>(1024*1024);
        auto response = httpClient.request("PUT", "/job/apiv1/file/upload/", 
            nlohmann::json{{"jobId", jobId}, {"targetPath", "/data/myfile.png"}}.dump(),
            {{"Authorization", jwtToken.signature()}, {"Content-Length", std::to_string(fileData.size())}});

        // Check that the upload was successful
        BOOST_CHECK_EQUAL(std::stoi(response->status_code), 200);

        auto result = nlohmann::json::parse(response->content.string());
        BOOST_CHECK(result.contains("uploadId"));
        BOOST_CHECK_EQUAL(result["status"], "completed");

        sendDataThread.join();
        websocketFileUploadClient->stop();
        fileUploadThread->join();
    }

    BOOST_AUTO_TEST_CASE(test_file_upload_error_handling) {
        fileUploadCallback = [&](const Message& msg, const std::shared_ptr<TestWsClient::Connection>& connection) {
            // Use the ready message as our prompt to start sending file data
            if (msg.getId() != SERVER_READY) {
                BOOST_FAIL("File Upload client got unexpected message id " + std::to_string(msg.getId()));
                return;
            }

            // Send an error message instead of file details
            auto errorMsg = Message(FILE_UPLOAD_ERROR, Message::Priority::Highest, "");
            errorMsg.push_string("File upload failed: Permission denied");
            sendMessage(&errorMsg, connection);
        };

        this->startWebSocketClient();
        readyPromise.get_future().wait();

        // Create a file upload ID
        auto uploadId = this->requestFileUploadId();

        // Try to upload the file
        auto response = httpClient.request("PUT", "/job/apiv1/file/upload/", 
            nlohmann::json{{"jobId", jobId}, {"targetPath", "/data/myfile.png"}}.dump(),
            {{"Authorization", jwtToken.signature()}, {"Content-Length", "1024"}});

        // Check that the upload failed with the expected error
        BOOST_CHECK_EQUAL(std::stoi(response->status_code), 400);
        BOOST_CHECK(response->content.string().find("Permission denied") != std::string::npos);

        websocketFileUploadClient->stop();
        fileUploadThread->join();
    }

    BOOST_AUTO_TEST_CASE(test_file_upload_unauthorized) {
        // Try to upload without proper authorization
        auto response = httpClient.request("PUT", "/job/apiv1/file/upload/", 
            nlohmann::json{{"jobId", jobId}, {"targetPath", "/data/myfile.png"}}.dump(),
            {{"Content-Length", "1024"}});

        // Check that the upload was rejected
        BOOST_CHECK_EQUAL(std::stoi(response->status_code), 403);
    }

    BOOST_AUTO_TEST_CASE(test_file_upload_missing_parameters) {
        setJwtSecret(std::static_pointer_cast<HttpServer>(httpServer)->getvJwtSecrets()->back().secret());

        // Try to upload without required parameters
        auto response = httpClient.request("PUT", "/job/apiv1/file/upload/", 
            nlohmann::json{{"jobId", jobId}}.dump(),
            {{"Authorization", jwtToken.signature()}, {"Content-Length", "1024"}});

        // Check that the upload was rejected
        BOOST_CHECK_EQUAL(std::stoi(response->status_code), 400);
    }

    BOOST_AUTO_TEST_CASE(test_file_upload_invalid_cluster) {
        setJwtSecret(std::static_pointer_cast<HttpServer>(httpServer)->getvJwtSecrets()->back().secret());

        // Try to upload to invalid cluster
        auto response = httpClient.request("PUT", "/job/apiv1/file/upload/", 
            nlohmann::json{{"cluster", "invalid_cluster"}, {"bundle", "whatever"}, {"targetPath", "/data/myfile.png"}}.dump(),
            {{"Authorization", jwtToken.signature()}, {"Content-Length", "1024"}});

        // Check that the upload was rejected
        BOOST_CHECK_EQUAL(response->status_code, "400");
    }

BOOST_AUTO_TEST_SUITE_END()
