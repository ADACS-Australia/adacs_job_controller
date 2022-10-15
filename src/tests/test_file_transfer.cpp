//
// Created by lewis on 10/6/21.
//

#include "../../tests/utils.h"
#include "../../Cluster/ClusterManager.h"
#include "../../tests/fixtures/DatabaseFixture.h"
#include "../../tests/fixtures/HttpClientFixture.h"
#include "../../tests/fixtures/WebSocketClientFixture.h"
#include <boost/test/unit_test.hpp>
#include <cstddef>

// TODO(lewis): parseLine and getCurrentMemoryUsage functions require a refactor
auto parseLine(char* line) -> size_t{
    // This assumes that a digit will be found and the line ends in " Kb".
    size_t lineLen = strlen(line);
    const char* ptr = line;
    while (*ptr < '0' || *ptr > '9') { ptr++; } // NOLINT(cppcoreguidelines-pro-bounds-pointer-arithmetic)
    line[lineLen - 3] = '\0'; // NOLINT(cppcoreguidelines-pro-bounds-pointer-arithmetic)
    lineLen = std::atol(ptr); // NOLINT(cert-err34-c)
    return lineLen;
}

// NOLINTBEGIN(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
auto getCurrentMemoryUsage() -> size_t {
    FILE* file = fopen("/proc/self/status", "r"); // NOLINT(cppcoreguidelines-owning-memory)
    size_t result = -1;
    std::array<char, 128> line{};

    while (fgets(line.data(), 128, file) != nullptr){
        if (strncmp(line.data(), "VmRSS:", 6) == 0){
            result = parseLine(line.data());
            break;
        }
    }
    fclose(file); // NOLINT(cppcoreguidelines-owning-memory,cert-err33-c)
    return result;
}
// NOLINTEND(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)


struct FileTransferTestDataFixture : public DatabaseFixture, public WebSocketClientFixture, public HttpClientFixture {
    std::shared_ptr<Cluster> cluster;
    uint64_t jobId;

    FileTransferTestDataFixture() :
            cluster(clusterManager->getvClusters()->front())
    {
        jobId = database->run(
                insert_into(jobTable)
                        .set(
                                jobTable.user = 1,
                                jobTable.parameters = "params1",
                                jobTable.cluster = cluster->getName(),
                                jobTable.bundle = "whatever",
                                jobTable.application = httpServer->getvJwtSecrets()->back().name()
                        )
        );
    }
};

BOOST_FIXTURE_TEST_SUITE(file_transfer_test_suite, FileTransferTestDataFixture)
    BOOST_AUTO_TEST_CASE(test_file_transfer) { // NOLINT(readability-function-cognitive-complexity)
        std::shared_ptr<TestWsClient> websocketFileDownloadClient;
        auto fileData = std::vector<uint8_t>();
        bool bPaused = false;
        std::promise<void> bReady;
        std::thread pThread;
        std::shared_ptr<std::thread> fileDownloadThread;
        websocketClient->on_message = [&pThread, &fileData, &bPaused, &bReady, &fileDownloadThread, &websocketFileDownloadClient](const std::shared_ptr<TestWsClient::Connection>& /*connection*/, const std::shared_ptr<TestWsClient::InMessage>& in_message) {
            auto data = in_message->string();
            auto msg = Message(std::vector<uint8_t>(data.begin(), data.end()));

            // Ignore the ready message
            if (msg.getId() == SERVER_READY) {
                bReady.set_value();
                return;
            }

            if (msg.getId() == DOWNLOAD_FILE) {
                auto jobId = msg.pop_uint();
                auto uuid = msg.pop_string();
                auto sBundle = msg.pop_string();
                auto sFilePath = msg.pop_string();

                websocketFileDownloadClient = std::make_shared<TestWsClient>("localhost:8001/job/ws/?token=" + uuid);
                websocketFileDownloadClient->on_message = [&pThread, &fileData, &bPaused](const std::shared_ptr<TestWsClient::Connection>& connection, const std::shared_ptr<TestWsClient::InMessage>& in_message) {
                    auto data = in_message->string();
                    auto msg = Message(std::vector<uint8_t>(data.begin(), data.end()));

                    // Check if this is a pause transfer message
                    if (msg.getId() == PAUSE_FILE_CHUNK_STREAM) {
                        bPaused = true;
                        return;
                    }

                    // Check if this is a resume transfer message
                    if (msg.getId() == RESUME_FILE_CHUNK_STREAM) {
                        bPaused = false;
                        return;
                    }

                    // Use the ready message as our prompt to start sending file data
                    if (msg.getId() != SERVER_READY) {
                        BOOST_FAIL("File Download client got unexpected message id " + std::to_string(msg.getId()));
                        return;
                    }

                    // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
                    auto fileSize = randomInt(0, 1024ULL*1024ULL);
                    fileData.reserve(fileSize);

                    // Send the file size to the server
                    msg = Message(FILE_DETAILS, Message::Priority::Highest, "");
                    msg.push_ulong(fileSize);

                    auto outMessage = std::make_shared<TestWsClient::OutMessage>(msg.getdata()->get()->size());
                    std::ostream_iterator<uint8_t> iter(*outMessage);
                    std::copy(msg.getdata()->get()->begin(), msg.getdata()->get()->end(), iter);
                    // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
                    connection->send(outMessage, nullptr, 130);

                    // Now send the file content in to chunks and send it to the client
                    pThread = std::thread([&bPaused, connection, fileSize, &fileData]() {
                        // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
                        auto CHUNK_SIZE = 1024*64;

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

                            auto msg = Message(FILE_CHUNK, Message::Priority::Lowest, "");
                            msg.push_bytes(*data);

                            auto message = std::make_shared<TestWsClient::OutMessage>(msg.getdata()->get()->size());
                            std::ostream_iterator<uint8_t> iter(*message);
                            std::copy(msg.getdata()->get()->begin(), msg.getdata()->get()->end(), iter);
                            // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
                            connection->send(message, nullptr, 130);
                        }
                    });

                };

                // Start the client
                fileDownloadThread = std::make_shared<std::thread>([&websocketFileDownloadClient]() {
                    websocketFileDownloadClient->start();
                });

                return;
            }

            BOOST_FAIL("Master client got unexpected message id " + std::to_string(msg.getId()));
        };

        startWebSocketClient();
        bReady.get_future().wait();

        // Create a file download
        setJwtSecret(httpServer->getvJwtSecrets()->back().secret());

        // Create params
        nlohmann::json params = {
                {"jobId", jobId},
                {"path",  "/data/myfile.png"}
        };

        auto response = httpClient.request("POST", "/job/apiv1/file/", params.dump(), {{"Authorization", jwtToken.signature()}});

        nlohmann::json result;
        response->content >> result;

        // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
        for (int i = 0; i < 5; i++) {
            // Try to download the file
            response = httpClient.request("GET", "/job/apiv1/file/?fileId=" + std::string{result["fileId"]});
            auto content = response->content.string();

            auto returnData = std::vector<uint8_t>(content.begin(), content.end());

            // Check that the data collected by the client was correct
            BOOST_CHECK_EQUAL_COLLECTIONS(returnData.begin(), returnData.end(), fileData.begin(), fileData.end());

            fileData.clear();

            pThread.join();
            websocketFileDownloadClient->stop();
            fileDownloadThread->join();
        }
    }

    BOOST_AUTO_TEST_CASE(test_large_file_transfers) { // NOLINT(readability-function-cognitive-complexity)
        std::shared_ptr<TestWsClient> websocketFileDownloadClient;
        bool bPaused = false;
        std::promise<void> bReady;
        int pauseCount = 0;
        int resumeCount = 0;
        uint64_t fileSize = 0;
        std::shared_ptr<std::thread> pThread;
        std::shared_ptr<std::thread> fileDownloadThread;
        websocketClient->on_message = [this, &pThread, &bPaused, &bReady, &fileSize, &pauseCount, &resumeCount, &websocketFileDownloadClient, &fileDownloadThread](const std::shared_ptr<TestWsClient::Connection>& /*connection*/, const std::shared_ptr<TestWsClient::InMessage>& in_message) {
            auto data = in_message->string();
            auto msg = Message(std::vector<uint8_t>(data.begin(), data.end()));

            // Ignore the ready message
            if (msg.getId() == SERVER_READY) {
                bReady.set_value();
                return;
            }

            if (msg.getId() == DOWNLOAD_FILE) {
                auto jobId = msg.pop_uint();
                auto uuid = msg.pop_string();
                auto sBundle = msg.pop_string();
                auto sFilePath = msg.pop_string();

                websocketFileDownloadClient = std::make_shared<TestWsClient>("localhost:8001/job/ws/?token=" + uuid);
                websocketFileDownloadClient->on_message = [this, &pThread, &bPaused, &pauseCount, &resumeCount, &fileSize](const std::shared_ptr<TestWsClient::Connection> &connection, const std::shared_ptr<TestWsClient::InMessage> &in_message) {
                    auto data = in_message->string();
                    auto msg = Message(std::vector<uint8_t>(data.begin(), data.end()));

                    // Check if this is a pause transfer message
                    if (msg.getId() == PAUSE_FILE_CHUNK_STREAM) {
                        pauseCount++;
                        bPaused = true;
                        return;
                    }

                    // Check if this is a resume transfer message
                    if (msg.getId() == RESUME_FILE_CHUNK_STREAM) {
                        resumeCount++;
                        bPaused = false;
                        return;
                    }

                    // Use the ready message as our prompt to start sending file data
                    if (msg.getId() != SERVER_READY) {
                        BOOST_FAIL("File Download client got unexpected message id " + std::to_string(msg.getId()));
                        return;
                    }

                    // Generate a file size between 512 and 1024Mb
                    // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
                    fileSize = randomInt(1024ULL * 1024ULL * 512ULL, 1024ULL * 1024ULL * 1024ULL);

                    // Send the file size to the server
                    msg = Message(FILE_DETAILS, Message::Priority::Highest, "");
                    msg.push_ulong(fileSize);

                    auto smsg = Message(**msg.getdata());
                    clusterManager->getmConnectedFileDownloads()->begin()->second->callhandleMessage(smsg);

                    // Now send the file content in to chunks and send it to the client
                    pThread = std::make_shared<std::thread>([this, &bPaused, connection, fileSize]() {
                        // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
                        auto CHUNK_SIZE = 1024 * 64;

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

                            auto msg = Message(FILE_CHUNK, Message::Priority::Lowest, "");
                            msg.push_bytes(data);

                            auto smsg = Message(**msg.getdata());
                            clusterManager->getmConnectedFileDownloads()->begin()->second->callhandleMessage(smsg);
                        }
                    });
                };

                // Start the client
                fileDownloadThread = std::make_shared<std::thread>([&websocketFileDownloadClient]() {
                    websocketFileDownloadClient->start();
                });

                return;
            }

            BOOST_FAIL("Master client got unexpected message id " + std::to_string(msg.getId()));
        };

        startWebSocketClient();
        bReady.get_future().wait();

        // Create a file download
        setJwtSecret(httpServer->getvJwtSecrets()->back().secret());

        // Create params
        nlohmann::json params = {
                {"jobId", jobId},
                {"path",  "/data/myfile.png"}
        };

        auto response = httpClient.request("POST", "/job/apiv1/file/", params.dump(), {{"Authorization", jwtToken.signature()}});

        nlohmann::json result;
        response->content >> result;

        // Try to download the file
        auto baselineMemUsage = static_cast<int64_t>(getCurrentMemoryUsage());
        uint64_t totalBytesReceived = 0;
        bool end = false;
        // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
        httpClient.config.max_response_streambuf_size = static_cast<std::size_t>(1024*1024);
        httpClient.request(
            "GET",
            "/job/apiv1/file/?fileId=" + std::string{result["fileId"]},
            [&totalBytesReceived, &end](const std::shared_ptr<TestHttpClient::Response>& response, const SimpleWeb::error_code &) {
                totalBytesReceived += response->content.size();
                end = response->content.end;
                std::this_thread::sleep_for(std::chrono::milliseconds(1));
            }
        );

        // While the file download hasn't finished, check that the memory usage never exceeds 200Mb
        while (!end) {
            auto memUsage = static_cast<int64_t>(getCurrentMemoryUsage());
            // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
            if (memUsage - baselineMemUsage > static_cast<int64_t>(1024*200)) {
                BOOST_ASSERT_MSG(false, "Maximum tolerable memory usage was exceeded");
            }

            std::this_thread::sleep_for(std::chrono::milliseconds(1));
        }

        // Check that the total bytes received matches the total bytes sent
        BOOST_CHECK_EQUAL(fileSize, totalBytesReceived);

        pThread->join();
        websocketFileDownloadClient->stop();
        fileDownloadThread->join();

        std::cout << "Large file test complete. File size was " << fileSize << ". Pauses: " << pauseCount << ", resumes: " << resumeCount << std::endl;
        BOOST_CHECK_EQUAL(pauseCount > 0, true);
        BOOST_CHECK_EQUAL(pauseCount, resumeCount);
    }
BOOST_AUTO_TEST_SUITE_END()
