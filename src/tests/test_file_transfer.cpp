//
// Created by lewis on 10/6/21.
//

import settings;
#include <cstddef>

#include <boost/test/unit_test.hpp>
#include <jwt/jwt.hpp>
#include <sqlpp11/sqlpp11.h>

#include "../../Lib/shims/sqlpp_shim.h"
#include "../../tests/fixtures/DatabaseFixture.h"
#include "../../tests/fixtures/HttpClientFixture.h"
#include "../../tests/fixtures/WebSocketClientFixture.h"
#include "../../tests/utils.h"
import ClusterManager;
import Cluster;
import Message;
import HttpServer;

// TODO(lewis): parseLine and getCurrentMemoryUsage functions require a refactor
auto parseLine(char* line) -> size_t
{
    // This assumes that a digit will be found and the line ends in " Kb".
    size_t lineLen  = strlen(line);
    const char* ptr = line;
    while (*ptr < '0' || *ptr > '9')
    {
        ptr++;
    }  // NOLINT(cppcoreguidelines-pro-bounds-pointer-arithmetic)
    line[lineLen - 3] = '\0';            // NOLINT(cppcoreguidelines-pro-bounds-pointer-arithmetic)
    lineLen           = std::atol(ptr);  // NOLINT(cert-err34-c)
    return lineLen;
}

// NOLINTBEGIN(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
auto getCurrentMemoryUsage() -> size_t
{
    FILE* file    = fopen("/proc/self/status", "r");  // NOLINT(cppcoreguidelines-owning-memory)
    size_t result = -1;
    std::array<char, 128> line{};

    while (fgets(line.data(), 128, file) != nullptr)
    {
        if (strncmp(line.data(), "VmRSS:", 6) == 0)
        {
            result = parseLine(line.data());
            break;
        }
    }
    fclose(file);  // NOLINT(cppcoreguidelines-owning-memory,cert-err33-c)
    return result;
}

// NOLINTEND(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)


struct FileTransferTestDataFixture : public DatabaseFixture, public WebSocketClientFixture, public HttpClientFixture
{
    std::shared_ptr<Cluster> cluster;
    uint64_t jobId;
    std::promise<void> readyPromise;
    std::function<void(const Message&, const std::shared_ptr<TestWsClient::Connection>&)> fileDownloadCallback;
    std::shared_ptr<TestWsClient> websocketFileDownloadClient;
    std::shared_ptr<std::jthread> fileDownloadThread;
    std::vector<uint8_t> fileData;
    bool bPaused = false;
    std::jthread sendDataThread;

    FileTransferTestDataFixture()
        : cluster(std::static_pointer_cast<ClusterManager>(clusterManager)->getvClusters()->front())
    {
        jobId = database->run(insert_into(jobTable).set(
            jobTable.user        = 1,
            jobTable.parameters  = "params1",
            jobTable.cluster     = cluster->getName(),
            jobTable.bundle      = "whatever",
            jobTable.application = std::static_pointer_cast<HttpServer>(httpServer)->getvJwtSecrets()->back().name()));

        websocketClient->on_message = [this](const std::shared_ptr<TestWsClient::Connection>& connection,
                                             const std::shared_ptr<TestWsClient::InMessage>& in_message) {
            onWebsocketMessage(connection, in_message);
        };
    }

    ~FileTransferTestDataFixture()
    {
        if (websocketFileDownloadClient)
        {
            websocketFileDownloadClient->stop();
        }
    }

    FileTransferTestDataFixture(FileTransferTestDataFixture const&)                    = delete;
    auto operator=(FileTransferTestDataFixture const&) -> FileTransferTestDataFixture& = delete;
    FileTransferTestDataFixture(FileTransferTestDataFixture&&)                         = delete;
    auto operator=(FileTransferTestDataFixture&&) -> FileTransferTestDataFixture&      = delete;

    void onWebsocketMessage(const std::shared_ptr<TestWsClient::Connection>& /*connection*/,
                            const std::shared_ptr<TestWsClient::InMessage>& in_message)
    {
        auto data = in_message->string();
        auto msg  = Message(std::vector<uint8_t>(data.begin(), data.end()));

        // Ignore the ready message
        if (msg.getId() == SERVER_READY)
        {
            readyPromise.set_value();
            return;
        }

        if (msg.getId() == DOWNLOAD_FILE)
        {
            msg.pop_uint();
            auto uuid = msg.pop_string();
            msg.pop_string();
            msg.pop_string();

            websocketFileDownloadClient = std::make_shared<TestWsClient>("localhost:8001/job/ws/?token=" + uuid);
            websocketFileDownloadClient->on_message =
                [this](const std::shared_ptr<TestWsClient::Connection>& connection,
                       const std::shared_ptr<TestWsClient::InMessage>& in_message) {
                    auto data = in_message->string();
                    auto msg  = Message(std::vector<uint8_t>(data.begin(), data.end()));
                    fileDownloadCallback(msg, connection);
                };

            // Start the client
            fileDownloadThread = std::make_shared<std::jthread>([this]() {
                websocketFileDownloadClient->start();
            });

            return;
        }

        BOOST_FAIL("Master client got unexpected message id " + std::to_string(msg.getId()));
    }

    auto requestFileDownloadId() -> std::string
    {
        // Create a file download
        setJwtSecret(std::static_pointer_cast<HttpServer>(httpServer)->getvJwtSecrets()->back().secret());

        // Create params
        nlohmann::json params = {
            {"jobId",              jobId},
            { "path", "/data/myfile.png"}
        };

        auto response = httpClient.request("POST",
                                           "/job/apiv1/file/",
                                           params.dump(),
                                           {
                                               {"Authorization", jwtToken.signature()}
        });

        nlohmann::json result;
        response->content >> result;

        return {result["fileId"]};
    }

    static void sendMessage(Message* msg, const std::shared_ptr<TestWsClient::Connection>& connection)
    {
        auto message = std::make_shared<TestWsClient::OutMessage>(msg->getdata()->get()->size());
        std::ostream_iterator<uint8_t> iter(*message);
        std::copy(msg->getdata()->get()->begin(), msg->getdata()->get()->end(), iter);
        // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
        connection->send(message, nullptr, 130);
    }
};

BOOST_FIXTURE_TEST_SUITE(file_transfer_test_suite, FileTransferTestDataFixture)

BOOST_AUTO_TEST_CASE(test_file_transfer)
{
    fileDownloadCallback = [&](const Message& msg, const std::shared_ptr<TestWsClient::Connection>& connection) {
        // Check if this is a pause transfer message
        if (msg.getId() == PAUSE_FILE_CHUNK_STREAM)
        {
            bPaused = true;
            return;
        }

        // Check if this is a resume transfer message
        if (msg.getId() == RESUME_FILE_CHUNK_STREAM)
        {
            bPaused = false;
            return;
        }

        // Use the ready message as our prompt to start sending file data
        if (msg.getId() != SERVER_READY)
        {
            BOOST_FAIL("File Download client got unexpected message id " + std::to_string(msg.getId()));
            return;
        }

        // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
        auto fileSize = randomInt(0, 1024ULL * 1024ULL);
        fileData.reserve(fileSize);

        // Send the file size to the server
        auto newmsg = Message(FILE_DETAILS, Message::Priority::Highest, "");
        newmsg.push_ulong(fileSize);

        sendMessage(&newmsg, connection);

        // Now send the file content in to chunks and send it to the client
        sendDataThread = std::jthread([this, connection, fileSize]() {
            // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
            auto CHUNK_SIZE = 1024 * 64;

            uint64_t bytesSent = 0;
            while (bytesSent < fileSize)
            {
                // Don't do anything while the stream is paused
                while (bPaused)
                {
                    std::this_thread::yield();
                }

                auto chunkSize =
                    std::min(static_cast<uint32_t>(CHUNK_SIZE), static_cast<uint32_t>(fileSize - bytesSent));
                bytesSent += chunkSize;

                auto data = generateRandomData(chunkSize);

                fileData.insert(fileData.end(), (*data).begin(), (*data).end());

                auto msg = Message(FILE_CHUNK, Message::Priority::Lowest, "");
                msg.push_bytes(*data);

                sendMessage(&msg, connection);
            }
        });
    };

    startWebSocketClient();
    readyPromise.get_future().wait();

    // Create a file download ID
    auto fileId = requestFileDownloadId();

    // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
    for (int i = 0; i < 5; i++)
    {
        // Try to download the file
        auto response = httpClient.request("GET", "/job/apiv1/file/?fileId=" + fileId);
        auto content  = response->content.string();

        auto returnData = std::vector<uint8_t>(content.begin(), content.end());

        // Check that the data collected by the client was correct
        BOOST_CHECK_EQUAL_COLLECTIONS(returnData.begin(), returnData.end(), fileData.begin(), fileData.end());

        fileData.clear();

        sendDataThread.join();
        websocketFileDownloadClient->stop();
        fileDownloadThread->join();
    }
}

BOOST_AUTO_TEST_CASE(test_large_file_transfers)
{  // NOLINT(readability-function-cognitive-complexity)
    int pauseCount       = 0;
    int resumeCount      = 0;
    uint64_t fileSize    = 0;
    fileDownloadCallback = [&, this](const Message& msg, const std::shared_ptr<TestWsClient::Connection>& connection) {
        // Check if this is a pause transfer message
        if (msg.getId() == PAUSE_FILE_CHUNK_STREAM)
        {
            pauseCount++;
            bPaused = true;
            return;
        }

        // Check if this is a resume transfer message
        if (msg.getId() == RESUME_FILE_CHUNK_STREAM)
        {
            resumeCount++;
            bPaused = false;
            return;
        }

        // Use the ready message as our prompt to start sending file data
        if (msg.getId() != SERVER_READY)
        {
            BOOST_FAIL("File Download client got unexpected message id " + std::to_string(msg.getId()));
            return;
        }

        // Generate a file size between 512 and 1024Mb
        // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
        fileSize = randomInt(1024ULL * 1024ULL * 512ULL, 1024ULL * 1024ULL * 1024ULL);

        // Send the file size to the server
        auto newmsg = Message(FILE_DETAILS, Message::Priority::Highest, "");
        newmsg.push_ulong(fileSize);

        auto smsg = Message(**newmsg.getdata());
        std::static_pointer_cast<ClusterManager>(clusterManager)
            ->getmConnectedFileDownloads()
            ->begin()
            ->second->callhandleMessage(smsg);

        // Now send the file content in to chunks and send it to the client
        sendDataThread = std::jthread([this, connection, fileSize]() {
            // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
            auto CHUNK_SIZE = 1024 * 64;

            auto data = std::vector<uint8_t>();

            uint64_t bytesSent = 0;
            while (bytesSent < fileSize)
            {
                // Spin while the stream is paused
                while (bPaused)
                {
                    std::this_thread::yield();
                }

                auto chunkSize =
                    std::min(static_cast<uint64_t>(CHUNK_SIZE), static_cast<uint64_t>(fileSize - bytesSent));
                bytesSent += chunkSize;
                data.resize(chunkSize);

                auto msg = Message(FILE_CHUNK, Message::Priority::Lowest, "");
                msg.push_bytes(data);

                auto smsg = Message(**msg.getdata());
                std::static_pointer_cast<ClusterManager>(clusterManager)
                    ->getmConnectedFileDownloads()
                    ->begin()
                    ->second->callhandleMessage(smsg);
            }
        });
    };

    startWebSocketClient();
    readyPromise.get_future().wait();

    // Create a file download ID
    auto fileId = requestFileDownloadId();

    // Try to download the file
    auto baselineMemUsage                         = static_cast<int64_t>(getCurrentMemoryUsage());
    uint64_t totalBytesReceived                   = 0;
    bool end                                      = false;
    // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
    httpClient.config.max_response_streambuf_size = static_cast<std::size_t>(1024 * 1024);
    httpClient.request("GET",
                       "/job/apiv1/file/?fileId=" + fileId,
                       [&totalBytesReceived, &end](const std::shared_ptr<TestHttpClient::Response>& response,
                                                   const SimpleWeb::error_code&) {
                           totalBytesReceived += response->content.size();
                           end                 = response->content.end;
                           std::this_thread::sleep_for(std::chrono::milliseconds(1));
                       });

    // While the file download hasn't finished, check that the memory usage never exceeds 200Mb
    while (!end)
    {
        auto memUsage = static_cast<int64_t>(getCurrentMemoryUsage());
        // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
        if (memUsage - baselineMemUsage > static_cast<int64_t>(1024 * 200))
        {
            BOOST_ASSERT_MSG(false, "Maximum tolerable memory usage was exceeded");
        }

        std::this_thread::sleep_for(std::chrono::milliseconds(1));
    }

    // Check that the total bytes received matches the total bytes sent
    BOOST_CHECK_EQUAL(fileSize, totalBytesReceived);

    std::cout << "Large file test complete. File size was " << fileSize << ". Pauses: " << pauseCount
              << ", resumes: " << resumeCount << std::endl;
    BOOST_CHECK_EQUAL(pauseCount > 0, true);
    BOOST_CHECK_EQUAL(pauseCount, resumeCount);
}

BOOST_AUTO_TEST_CASE(test_file_transfer_connection_timeout)
{
    fileDownloadCallback = [&](const Message& msg, const std::shared_ptr<TestWsClient::Connection>& connection) {};

    startWebSocketClient();
    readyPromise.get_future().wait();

    auto fileId = requestFileDownloadId();

    auto response = httpClient.request("GET", "/job/apiv1/file/?fileId=" + fileId);
    auto content  = response->content.string();

    BOOST_CHECK_EQUAL(std::stoi(response->status_code),
                      static_cast<int>(SimpleWeb::StatusCode::client_error_bad_request));
    BOOST_CHECK_EQUAL(content, "Client took too long to respond.");
}

BOOST_AUTO_TEST_CASE(test_file_transfer_error)
{
    fileDownloadCallback = [&](const Message& /*msg*/, const std::shared_ptr<TestWsClient::Connection>& connection) {
        auto result = Message(FILE_ERROR, Message::Priority::Lowest, "");
        result.push_string("Error test");

        sendMessage(&result, connection);
    };

    startWebSocketClient();
    readyPromise.get_future().wait();

    auto fileId = requestFileDownloadId();

    auto response = httpClient.request("GET", "/job/apiv1/file/?fileId=" + fileId);
    auto content  = response->content.string();

    BOOST_CHECK_EQUAL(std::stoi(response->status_code),
                      static_cast<int>(SimpleWeb::StatusCode::client_error_bad_request));
    BOOST_CHECK_EQUAL(content, "Error test");
}

BOOST_AUTO_TEST_CASE(test_file_transfer_no_details)
{
    fileDownloadCallback = [&](const Message& /*msg*/, const std::shared_ptr<TestWsClient::Connection>& connection) {
        auto result = Message(FILE_CHUNK, Message::Priority::Lowest, "");
        result.push_bytes({0, 1, 2});

        sendMessage(&result, connection);
    };

    startWebSocketClient();
    readyPromise.get_future().wait();

    auto fileId = requestFileDownloadId();

    auto response = httpClient.request("GET", "/job/apiv1/file/?fileId=" + fileId);
    auto content  = response->content.string();

    BOOST_CHECK_EQUAL(std::stoi(response->status_code),
                      static_cast<int>(SimpleWeb::StatusCode::server_error_service_unavailable));
    BOOST_CHECK_EQUAL(content, "Remote Cluster Offline");
}

BOOST_AUTO_TEST_CASE(test_file_transfer_data_timeout)
{
    fileDownloadCallback = [&](const Message& /*msg*/, const std::shared_ptr<TestWsClient::Connection>& connection) {
        auto result = Message(FILE_DETAILS, Message::Priority::Lowest, "");
        // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
        result.push_ulong(12345);

        sendMessage(&result, connection);
    };

    startWebSocketClient();
    readyPromise.get_future().wait();

    auto fileId = requestFileDownloadId();

    // Server will force close the connection in the case of an error
    BOOST_CHECK_THROW(httpClient.request("GET", "/job/apiv1/file/?fileId=" + fileId), boost::system::system_error);
}

BOOST_AUTO_TEST_CASE(test_file_transfer_websocket_connection_broken)
{
    fileDownloadCallback = [&](const Message& /*msg*/, const std::shared_ptr<TestWsClient::Connection>& connection) {
        {
            auto result = Message(FILE_DETAILS, Message::Priority::Lowest, "");
            // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
            result.push_ulong(12345);

            sendMessage(&result, connection);
        }
        {
            auto result = Message(FILE_CHUNK, Message::Priority::Lowest, "");
            result.push_bytes({0, 1, 2});

            sendMessage(&result, connection);
        }

        // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
        std::this_thread::sleep_for(std::chrono::milliseconds(100));
        connection->close();
    };

    startWebSocketClient();
    readyPromise.get_future().wait();

    auto fileId = requestFileDownloadId();

    // Server will force close the connection in the case of an error
    BOOST_CHECK_THROW(httpClient.request("GET", "/job/apiv1/file/?fileId=" + fileId), boost::system::system_error);
}

BOOST_AUTO_TEST_CASE(test_file_transfer_http_connection_broken)
{  // NOLINT(readability-function-cognitive-complexity)
    bool bRunning        = true;
    fileDownloadCallback = [&, this](const Message& msg, const std::shared_ptr<TestWsClient::Connection>& connection) {
        // Check if this is a pause transfer message
        if (msg.getId() == PAUSE_FILE_CHUNK_STREAM)
        {
            return;
        }

        // Check if this is a resume transfer message
        if (msg.getId() == RESUME_FILE_CHUNK_STREAM)
        {
            return;
        }

        // Use the ready message as our prompt to start sending file data
        if (msg.getId() != SERVER_READY)
        {
            BOOST_FAIL("File Download client got unexpected message id " + std::to_string(msg.getId()));
            return;
        }

        // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
        auto fileSize = 1024ULL * 1024ULL * 1024ULL;

        // Send the file size to the server
        auto newmsg = Message(FILE_DETAILS, Message::Priority::Highest, "");
        newmsg.push_ulong(fileSize);

        auto smsg = Message(**newmsg.getdata());
        std::static_pointer_cast<ClusterManager>(clusterManager)
            ->getmConnectedFileDownloads()
            ->begin()
            ->second->callhandleMessage(smsg);

        // Now send the file content in to chunks and send it to the client
        sendDataThread = std::jthread([this, connection, &bRunning]() {
            // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
            auto CHUNK_SIZE = 1024 * 64;

            auto data = std::vector<uint8_t>();
            data.resize(CHUNK_SIZE);

            while (bRunning)
            {
                auto msg = Message(FILE_CHUNK, Message::Priority::Lowest, "");
                msg.push_bytes(data);

                auto smsg = Message(**msg.getdata());
                if (!std::static_pointer_cast<ClusterManager>(clusterManager)->getmConnectedFileDownloads()->empty() &&
                    std::static_pointer_cast<ClusterManager>(clusterManager)
                            ->getmConnectedFileDownloads()
                            ->begin()
                            ->second != nullptr)
                {
                    std::static_pointer_cast<ClusterManager>(clusterManager)
                        ->getmConnectedFileDownloads()
                        ->begin()
                        ->second->callhandleMessage(smsg);
                }
            }
        });
    };

    startWebSocketClient();
    readyPromise.get_future().wait();

    // Create a file download ID
    auto fileId = requestFileDownloadId();

    // Try to download the file
    uint64_t totalBytesReceived                   = 0;
    bool end                                      = false;
    // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
    httpClient.config.max_response_streambuf_size = static_cast<std::size_t>(1024 * 1024);
    httpClient.request("GET",
                       "/job/apiv1/file/?fileId=" + fileId,
                       [&](const std::shared_ptr<TestHttpClient::Response>& response, const SimpleWeb::error_code&) {
                           totalBytesReceived += response->content.size();
                           end                 = response->content.end;

                           // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
                           if (totalBytesReceived > 1024ULL * 1024ULL * 128ULL)
                           {
                               response->close();
                               BOOST_CHECK_EQUAL(std::static_pointer_cast<ClusterManager>(clusterManager)
                                                         ->getmConnectedFileDownloads()
                                                         ->begin()
                                                         ->second->getpConnection() == nullptr,
                                                 false);
                               end = true;
                           }
                       });

    // Wait for the transfer to complete
    while (!end)
    {
        std::this_thread::sleep_for(std::chrono::milliseconds(1));
    }

    // Wait until the connected clusters becomes empty again (Connection closed)
    int count = 0;
    // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
    for (;
         count < 10 && !std::static_pointer_cast<ClusterManager>(clusterManager)->getmConnectedFileDownloads()->empty();
         count++)
    {
        // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
        std::this_thread::sleep_for(std::chrono::milliseconds(100));
    }

    // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
    if (count == 10)
    {
        BOOST_FAIL("Websocket connection didn't close when it should have.");
    }

    bRunning = false;
    sendDataThread.join();
}
BOOST_AUTO_TEST_SUITE_END()
