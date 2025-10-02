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
    std::function<void(Message&, const std::shared_ptr<TestWsClient::Connection>&)> fileUploadCallback;
    std::shared_ptr<TestWsClient> websocketFileUploadClient;
    std::shared_ptr<std::jthread> fileUploadThread;
    std::vector<uint8_t> fileData;
    uint64_t expectedFileSize = 0;
    bool bPaused = false;

    void handleFileUploadComplete(Message& msg, const std::shared_ptr<TestWsClient::Connection>& connection) {
        // Validate that we received the expected amount of data
        if (fileData.size() != expectedFileSize) {
            // Send FILE_UPLOAD_ERROR for truncated data
            std::string errorText = "Data truncated: expected " + std::to_string(expectedFileSize) + 
                " bytes, received " + std::to_string(fileData.size()) + " bytes";
            auto errorMsg = Message(FILE_UPLOAD_ERROR, Message::Priority::Highest, "");
            errorMsg.push_string(errorText);
            sendMessage(&errorMsg, connection);
        } else {
            // Data is complete - send FILE_UPLOAD_COMPLETE response back
            sendMessage(&msg, connection);
        }
    }
    
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
            expectedFileSize = msg.pop_ulong();

            websocketFileUploadClient = std::make_shared<TestWsClient>("localhost:8001/job/ws/?token=" + msg.getSource());
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
        connection->send(message, nullptr, 130);
    }

    auto generateRandomData(size_t size) -> std::shared_ptr<std::vector<uint8_t>> {
        auto data = std::make_shared<std::vector<uint8_t>>(size);
        for (size_t i = 0; i < size; i++) {
            (*data)[i] = static_cast<uint8_t>(rand() % 256);
        }
        return data;
    }

    auto randomInt(uint64_t min, uint64_t max) -> uint64_t {
        return min + (rand() % (max - min + 1));
    }
};

BOOST_FIXTURE_TEST_SUITE(file_upload_transfer_test_suite, FileUploadTransferTestDataFixture)
    BOOST_AUTO_TEST_CASE(test_file_upload_transfer) {
        
        fileUploadCallback = [&](Message& msg, const std::shared_ptr<TestWsClient::Connection>& connection) {
            if (msg.getId() == SERVER_READY) {
                // Connection is ready - send SERVER_READY response back to indicate we're ready
                sendMessage(&msg, connection);
                return;
            }
            
            if (msg.getId() == FILE_UPLOAD_CHUNK) {
                // Receive file chunk from server
                auto chunkData = msg.pop_bytes();
                fileData.insert(fileData.end(), chunkData.begin(), chunkData.end());
                return;
            }
            
            if (msg.getId() == FILE_UPLOAD_COMPLETE) {
                // Server finished sending file - validate size and respond appropriately
                handleFileUploadComplete(msg, connection);
                return;
            }
            
            BOOST_FAIL("File Upload client got unexpected message id " + std::to_string(msg.getId()));
        };

        this->startWebSocketClient();
        readyPromise.get_future().wait();

        // Generate random test file data to upload
        const uint64_t testFileSize = randomInt(512, 2048); // Random size between 512B-2KB
        auto originalFileData = generateRandomData(testFileSize);
        
        // Set JWT secret for authentication
        setJwtSecret(std::static_pointer_cast<HttpServer>(httpServer)->getvJwtSecrets()->back().secret());

        // Create request body with the file data
        std::string requestBody = std::string(originalFileData->begin(), originalFileData->end());
        
        // Create URL with query parameters for job ID and target path
        std::string uploadUrl = "/job/apiv1/file/upload/?jobId=" + std::to_string(jobId) + "&targetPath=/data/myfile.png";
        auto response = httpClient.request("PUT", uploadUrl, requestBody,
            {{"Authorization", jwtToken.signature()}, 
             {"Content-Type", "application/octet-stream"},
             {"Content-Length", std::to_string(testFileSize)}});

        // Check that the upload was successful
        auto responseContent = response->content.string();
        BOOST_CHECK_EQUAL(std::stoi(response->status_code), 200);
        
        auto result = nlohmann::json::parse(responseContent);
        BOOST_CHECK(result.contains("uploadId"));
        BOOST_CHECK_EQUAL(result["status"], "completed");

        // Verify that the client received the same data that was sent
        BOOST_CHECK_EQUAL(fileData.size(), originalFileData->size());
        BOOST_CHECK_EQUAL_COLLECTIONS(fileData.begin(), fileData.end(), 
                                    originalFileData->begin(), originalFileData->end());

        if (websocketFileUploadClient) {
            websocketFileUploadClient->stop();
        }
        if (fileUploadThread && fileUploadThread->joinable()) {
            fileUploadThread->join();
        }
    }

    BOOST_AUTO_TEST_CASE(test_large_file_uploads) {
        fileUploadCallback = [&](Message& msg, const std::shared_ptr<TestWsClient::Connection>& connection) {
            if (msg.getId() == SERVER_READY) {
                // Connection is ready - send SERVER_READY response back to indicate we're ready
                sendMessage(&msg, connection);
                return;
            }
            
            if (msg.getId() == FILE_UPLOAD_CHUNK) {
                // Receive file chunk from server
                auto chunkData = msg.pop_bytes();
                fileData.insert(fileData.end(), chunkData.begin(), chunkData.end());
                return;
            }
            
            if (msg.getId() == FILE_UPLOAD_COMPLETE) {
                // Server finished sending file - validate size and respond appropriately
                handleFileUploadComplete(msg, connection);
                return;
            }
            
            BOOST_FAIL("Large file upload client got unexpected message id " + std::to_string(msg.getId()));
        };

        this->startWebSocketClient();
        readyPromise.get_future().wait();

        // Generate large random test file data to upload
        const uint64_t testFileSize = randomInt(1024*1024, 5*1024*1024); // Random size between 1MB-5MB
        auto originalFileData = generateRandomData(testFileSize);

        // Set JWT secret for authentication
        setJwtSecret(std::static_pointer_cast<HttpServer>(httpServer)->getvJwtSecrets()->back().secret());

        // Try to upload the file
        auto baselineMemUsage = static_cast<int64_t>(getCurrentMemoryUsage());
        uint64_t totalBytesReceived = 0;
        bool end = false;
        httpClient.config.max_response_streambuf_size = static_cast<std::size_t>(1024*1024);
        
        // Create request body with the file data
        std::string requestBody = std::string(originalFileData->begin(), originalFileData->end());
        
        // Create URL with query parameters for job ID and target path
        std::string uploadUrl = "/job/apiv1/file/upload/?jobId=" + std::to_string(jobId) + "&targetPath=/data/myfile.png";
        auto response = httpClient.request("PUT", uploadUrl, requestBody,
            {{"Authorization", jwtToken.signature()}, 
             {"Content-Type", "application/octet-stream"},
             {"Content-Length", std::to_string(testFileSize)}});

        // Check that the upload was successful
        auto responseContent = response->content.string();
        BOOST_CHECK_EQUAL(std::stoi(response->status_code), 200);
        
        auto result = nlohmann::json::parse(responseContent);
        BOOST_CHECK(result.contains("uploadId"));
        BOOST_CHECK_EQUAL(result["status"], "completed");

        // Verify that the client received the same data that was sent
        BOOST_CHECK_EQUAL(fileData.size(), originalFileData->size());
        BOOST_CHECK_EQUAL_COLLECTIONS(fileData.begin(), fileData.end(), 
                                    originalFileData->begin(), originalFileData->end());

        // Check that memory usage didn't grow too much
        auto finalMemUsage = static_cast<int64_t>(getCurrentMemoryUsage());
        auto memGrowth = finalMemUsage - baselineMemUsage;
        BOOST_CHECK_LT(memGrowth, 50 * 1024 * 1024); // Should not grow by more than 50MB

        websocketFileUploadClient->stop();
        fileUploadThread->join();
    }

    BOOST_AUTO_TEST_CASE(test_continuous_file_uploads) {
        fileUploadCallback = [&](Message& msg, const std::shared_ptr<TestWsClient::Connection>& connection) {
            if (msg.getId() == SERVER_READY) {
                // Connection is ready - send SERVER_READY response back to indicate we're ready
                sendMessage(&msg, connection);
                return;
            }
            
            if (msg.getId() == FILE_UPLOAD_CHUNK) {
                // Receive file chunk from server
                auto chunkData = msg.pop_bytes();
                fileData.insert(fileData.end(), chunkData.begin(), chunkData.end());
                return;
            }
            
            if (msg.getId() == FILE_UPLOAD_COMPLETE) {
                // Server finished sending file - validate size and respond appropriately
                handleFileUploadComplete(msg, connection);
                return;
            }
            
            BOOST_FAIL("Continuous file upload client got unexpected message id " + std::to_string(msg.getId()));
        };

        this->startWebSocketClient();
        readyPromise.get_future().wait();

        // Generate random test file data for continuous upload
        const uint64_t testFileSize = randomInt(2*1024*1024, 10*1024*1024); // Random size between 2MB-10MB
        auto originalFileData = generateRandomData(testFileSize);

        // Set JWT secret for authentication
        setJwtSecret(std::static_pointer_cast<HttpServer>(httpServer)->getvJwtSecrets()->back().secret());

        // Try to upload the file
        uint64_t totalBytesReceived = 0;
        bool end = false;
        httpClient.config.max_response_streambuf_size = static_cast<std::size_t>(1024*1024);
        
        // Create request body with the file data
        std::string requestBody = std::string(originalFileData->begin(), originalFileData->end());
        
        // Create URL with query parameters for job ID and target path
        std::string uploadUrl = "/job/apiv1/file/upload/?jobId=" + std::to_string(jobId) + "&targetPath=/data/myfile.png";
        auto response = httpClient.request("PUT", uploadUrl, requestBody,
            {{"Authorization", jwtToken.signature()}, 
             {"Content-Type", "application/octet-stream"},
             {"Content-Length", std::to_string(testFileSize)}});

        // Check that the upload was successful
        auto responseContent = response->content.string();
        BOOST_CHECK_EQUAL(std::stoi(response->status_code), 200);
        
        auto result = nlohmann::json::parse(responseContent);
        BOOST_CHECK(result.contains("uploadId"));
        BOOST_CHECK_EQUAL(result["status"], "completed");

        // Verify that the client received the same data that was sent
        BOOST_CHECK_EQUAL(fileData.size(), originalFileData->size());
        BOOST_CHECK_EQUAL_COLLECTIONS(fileData.begin(), fileData.end(), 
                                    originalFileData->begin(), originalFileData->end());

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

        // Set JWT secret for authentication
        setJwtSecret(std::static_pointer_cast<HttpServer>(httpServer)->getvJwtSecrets()->back().secret());

        // Try to upload the file with query parameters
        std::string uploadUrl = "/job/apiv1/file/upload/?jobId=" + std::to_string(jobId) + "&targetPath=/data/myfile.png";
        std::string dummyFileData(1024, 'X'); // Create dummy file data
        auto response = httpClient.request("PUT", uploadUrl, dummyFileData,
            {{"Authorization", jwtToken.signature()}, 
             {"Content-Type", "application/octet-stream"},
             {"Content-Length", "1024"}});

        // Check that the upload failed with the expected error
        BOOST_CHECK_EQUAL(std::stoi(response->status_code), 400);
        BOOST_CHECK(response->content.string().find("Permission denied") != std::string::npos);

        websocketFileUploadClient->stop();
        fileUploadThread->join();
    }

    BOOST_AUTO_TEST_CASE(test_file_upload_unauthorized) {
        // Try to upload without proper authorization
        std::string uploadUrl = "/job/apiv1/file/upload/?jobId=" + std::to_string(jobId) + "&targetPath=/data/myfile.png";
        std::string dummyFileData(1024, 'X');
        auto response = httpClient.request("PUT", uploadUrl, dummyFileData,
            {{"Content-Type", "application/octet-stream"}, {"Content-Length", "1024"}});

        // Check that the upload was rejected
        BOOST_CHECK_EQUAL(std::stoi(response->status_code), 403);
    }

    BOOST_AUTO_TEST_CASE(test_file_upload_missing_parameters) {
        setJwtSecret(std::static_pointer_cast<HttpServer>(httpServer)->getvJwtSecrets()->back().secret());

        // Try to upload without required targetPath parameter (only jobId provided)
        std::string uploadUrl = "/job/apiv1/file/upload/?jobId=" + std::to_string(jobId);
        std::string dummyFileData(1024, 'X');
        auto response = httpClient.request("PUT", uploadUrl, dummyFileData,
            {{"Authorization", jwtToken.signature()}, 
             {"Content-Type", "application/octet-stream"}, 
             {"Content-Length", "1024"}});

        // Check that the upload was rejected
        BOOST_CHECK_EQUAL(std::stoi(response->status_code), 400);
    }

    BOOST_AUTO_TEST_CASE(test_file_upload_invalid_cluster) {
        setJwtSecret(std::static_pointer_cast<HttpServer>(httpServer)->getvJwtSecrets()->back().secret());

        // Try to upload to invalid cluster (using cluster and bundle parameters instead of jobId)
        std::string uploadUrl = "/job/apiv1/file/upload/?cluster=invalid_cluster&bundle=whatever&targetPath=/data/myfile.png";
        std::string dummyFileData(1024, 'X');
        auto response = httpClient.request("PUT", uploadUrl, dummyFileData,
            {{"Authorization", jwtToken.signature()}, 
             {"Content-Type", "application/octet-stream"}, 
             {"Content-Length", "1024"}});

        // Check that the upload was rejected
        BOOST_CHECK_EQUAL(std::stoi(response->status_code), 400);
    }

    BOOST_AUTO_TEST_CASE(test_file_upload_truncated_data) {
        // This test simulates the client side truncating data to test client-side validation
        fileUploadCallback = [&](Message& msg, const std::shared_ptr<TestWsClient::Connection>& connection) {
            if (msg.getId() == SERVER_READY) {
                sendMessage(&msg, connection);
                return;
            }
            
            if (msg.getId() == FILE_UPLOAD_CHUNK) {
                // Intentionally only receive partial data to simulate truncation
                auto chunkData = msg.pop_bytes();
                
                // Only collect first 512 bytes regardless of chunk size to simulate truncation
                if (fileData.size() < 512) {
                    size_t bytesToAdd = std::min(chunkData.size(), 512 - fileData.size());
                    fileData.insert(fileData.end(), chunkData.begin(), chunkData.begin() + bytesToAdd);
                }
                return;
            }
            
            if (msg.getId() == FILE_UPLOAD_COMPLETE) {
                // This should trigger our validation logic and send FILE_UPLOAD_ERROR
                handleFileUploadComplete(msg, connection);
                return;
            }
            
            BOOST_FAIL("Truncated data test client got unexpected message id " + std::to_string(msg.getId()));
        };

        this->startWebSocketClient();
        readyPromise.get_future().wait();

        // Generate test file data larger than 512 bytes to ensure truncation
        const uint64_t testFileSize = 1024; // 1KB file
        auto originalFileData = generateRandomData(testFileSize);

        // Set JWT secret for authentication
        setJwtSecret(std::static_pointer_cast<HttpServer>(httpServer)->getvJwtSecrets()->back().secret());

        // Upload the file
        std::string uploadUrl = "/job/apiv1/file/upload/?jobId=" + std::to_string(jobId) + "&targetPath=/data/truncated_test.bin";
        auto response = httpClient.request("PUT", uploadUrl, std::string(originalFileData->begin(), originalFileData->end()),
            {{"Authorization", jwtToken.signature()}, 
             {"Content-Type", "application/octet-stream"}, 
             {"Content-Length", std::to_string(testFileSize)}});

        // The HTTP response should return 400 Bad Request when client sends FILE_UPLOAD_ERROR
        BOOST_CHECK_EQUAL(response->status_code, "400 Bad Request"); // Should return 400 Bad Request when client sends FILE_UPLOAD_ERROR
        
        // Check that the response body contains the truncation error message
        std::string responseBody = response->content.string();
        BOOST_CHECK(responseBody.find("Data truncated") != std::string::npos); // Should contain our error message
        BOOST_CHECK(responseBody.find("expected 1024") != std::string::npos); // Should mention expected size
        BOOST_CHECK(responseBody.find("received 512") != std::string::npos); // Should mention actual size
        
        // Verify that our client truncated the data as expected
        BOOST_CHECK_EQUAL(fileData.size(), 512); // Should have truncated data (less than the expected 1024 bytes)
    }

    BOOST_AUTO_TEST_CASE(test_zero_byte_file_upload) {
        fileUploadCallback = [&](Message& msg, const std::shared_ptr<TestWsClient::Connection>& connection) {
            if (msg.getId() == SERVER_READY) {
                sendMessage(&msg, connection);
                return;
            }
            
            if (msg.getId() == FILE_UPLOAD_COMPLETE) {
                // For zero-byte file, we should get completion immediately without any chunks
                handleFileUploadComplete(msg, connection);
                return;
            }
            
            // Should never receive FILE_UPLOAD_CHUNK for a zero-byte file
            if (msg.getId() == FILE_UPLOAD_CHUNK) {
                BOOST_FAIL("Received FILE_UPLOAD_CHUNK for zero-byte file");
                return;
            }
            
            BOOST_FAIL("Zero-byte upload client got unexpected message id " + std::to_string(msg.getId()));
        };

        this->startWebSocketClient();
        readyPromise.get_future().wait();

        // Set JWT secret for authentication
        setJwtSecret(std::static_pointer_cast<HttpServer>(httpServer)->getvJwtSecrets()->back().secret());

        // Upload a zero-byte file
        std::string uploadUrl = "/job/apiv1/file/upload/?jobId=" + std::to_string(jobId) + "&targetPath=/data/empty.txt";
        std::string emptyData = ""; // Zero bytes
        auto response = httpClient.request("PUT", uploadUrl, emptyData,
            {{"Authorization", jwtToken.signature()}, 
             {"Content-Type", "application/octet-stream"}, 
             {"Content-Length", "0"}});

        // Check that the upload was successful
        BOOST_CHECK_EQUAL(std::stoi(response->status_code), 200);
        
        auto result = nlohmann::json::parse(response->content.string());
        BOOST_CHECK(result.contains("uploadId"));
        BOOST_CHECK_EQUAL(result["status"], "completed");

        // Verify no data was received (zero-byte file)
        BOOST_CHECK_EQUAL(fileData.size(), 0);
        
        websocketFileUploadClient->stop();
        fileUploadThread->join();
    }

BOOST_AUTO_TEST_SUITE_END()
