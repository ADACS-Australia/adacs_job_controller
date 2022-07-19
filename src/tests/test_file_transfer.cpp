//
// Created by lewis on 10/6/21.
//

#include "../Cluster/ClusterManager.h"
#include "jwt/jwt.hpp"
#include "utils.h"
#include <boost/test/unit_test.hpp>
#include <cstddef>

// NOLINTBEGIN(concurrency-mt-unsafe)

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

BOOST_AUTO_TEST_SUITE(file_transfer_test_suite)
    const auto sAccess = R"(
    [
        {
            "name": "app1",
            "secret": "super_secret1",
            "applications": [],
            "clusters": [
                "cluster1"
            ]
        }
    ]
    )";

    const auto sClusters = R"(
    [
        {
            "name": "cluster1",
            "host": "cluster1.com",
            "username": "user1",
            "path": "/cluster1/",
            "key": "cluster1_key"
        }
    ]
    )";

    BOOST_AUTO_TEST_CASE(test_file_transfer) {
        // Delete all database info just in case
        auto database = MySqlConnector();
        schema::JobserverFiledownload fileDownloadTable;
        schema::JobserverJob jobTable;
        schema::JobserverJobhistory jobHistoryTable;
        schema::JobserverClusteruuid clusterUuidTable;
        database->run(remove_from(fileDownloadTable).unconditionally());
        database->run(remove_from(jobHistoryTable).unconditionally());
        database->run(remove_from(jobTable).unconditionally());
        database->run(remove_from(clusterUuidTable).unconditionally());

        // Set up the cluster manager
        setenv(CLUSTER_CONFIG_ENV_VARIABLE, base64Encode(sClusters).c_str(), 1);
        auto mgr = std::make_shared<ClusterManager>();

        // Start the cluster scheduler
        bool running = true;
        std::thread clusterThread([&mgr, &running]() {
            while (running) {
                mgr->getvClusters()->at(0)->callrun();
            }
        });

        // Set up the test http server
        setenv(ACCESS_SECRET_ENV_VARIABLE, base64Encode(sAccess).c_str(), 1);
        auto httpSvr = std::make_shared<HttpServer>(mgr);
        httpSvr->start();

        // Set up the test websocket server
        auto wsSrv = WebSocketServer(mgr);
        wsSrv.start();

        BOOST_CHECK_EQUAL(acceptingConnections(8000), true);
        BOOST_CHECK_EQUAL(acceptingConnections(8001), true);

        // Try to reconnect the clusters so that we can get a connection token to use later to connect the client
        mgr->callreconnectClusters();

        // Connect a fake client to the websocket server
        TestWsClient websocketClient("localhost:8001/job/ws/?token=" + getLastToken());
        auto fileData = std::vector<uint8_t>();
        bool bPaused = false;
        bool bReady = false;
        std::thread pThread;
        websocketClient.on_message = [&pThread, &fileData, &bPaused, &bReady](const std::shared_ptr<TestWsClient::Connection>& connection, const std::shared_ptr<TestWsClient::InMessage>& in_message) {
            auto data = in_message->string();
            auto msg = Message(std::vector<uint8_t>(data.begin(), data.end()));

            // Ignore the ready message
            if (msg.getId() == SERVER_READY) {
                bReady = true;
                return;
            }

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

            msg.pop_uint();
            auto uuid = msg.pop_string();
            auto sBundle = msg.pop_string();
            auto sFilePath = msg.pop_string();

            // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
            auto fileSize = randomInt(0, 1024ULL*1024ULL);
            fileData.reserve(fileSize);

            // Send the file size to the server
            msg = Message(FILE_DETAILS, Message::Priority::Highest, "");
            msg.push_string(uuid);
            msg.push_ulong(fileSize);

            auto outMessage = std::make_shared<TestWsClient::OutMessage>(msg.getdata()->get()->size());
            std::ostream_iterator<uint8_t> iter(*outMessage);
            std::copy(msg.getdata()->get()->begin(), msg.getdata()->get()->end(), iter);
            // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
            connection->send(outMessage, nullptr, 130);

            // Now send the file content in to chunks and send it to the client
            pThread = std::thread([&bPaused, connection, fileSize, uuid, &fileData]() {
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
                    msg.push_string(uuid);
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
        std::thread clientThread([&websocketClient]() {
            websocketClient.start();
        });

        // Wait for the client to connect
        while (!bReady) {
            // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
            std::this_thread::sleep_for(std::chrono::milliseconds(10));
        }

        // Create a job to request files for
        auto jobId = database->run(
                insert_into(jobTable)
                        .set(
                                jobTable.user = 1,
                                jobTable.parameters = "params1",
                                jobTable.cluster = httpSvr->getvJwtSecrets()->at(0).clusters()[0],
                                jobTable.bundle = "whatever",
                                jobTable.application = httpSvr->getvJwtSecrets()->at(0).name()
                        )
        );

        // Create a file download
        // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
        auto now = std::chrono::system_clock::now() + std::chrono::minutes{10};
        jwt::jwt_object jwtToken = {
                jwt::params::algorithm("HS256"),
                jwt::params::payload({{"userName", "User"}}),
                jwt::params::secret(httpSvr->getvJwtSecrets()->at(0).secret())
        };
        jwtToken.add_claim("exp", now);

        // Since payload above only accepts string values, we need to set up any non-string values
        // separately
        // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
        jwtToken.payload().add_claim("userId", 5);

        // Create params
        nlohmann::json params = {
                {"jobId", jobId},
                {"path",  "/data/myfile.png"}
        };

        TestHttpClient httpClient("localhost:8000");
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
        }

        // Finished with the servers and clients
        running = false;
        *mgr->getvClusters()->at(0)->getdataReady() = true;
        mgr->getvClusters()->at(0)->getdataCV()->notify_one();
        clusterThread.join();
        websocketClient.stop();
        clientThread.join();
        httpSvr->stop();
        wsSrv.stop();

        // Clean up
        database->run(remove_from(fileDownloadTable).unconditionally());
        database->run(remove_from(jobHistoryTable).unconditionally());
        database->run(remove_from(jobTable).unconditionally());
    }

    BOOST_AUTO_TEST_CASE(test_large_file_transfers) { // NOLINT(readability-function-cognitive-complexity)
        // Delete all database info just in case
        auto database = MySqlConnector();
        schema::JobserverFiledownload fileDownloadTable;
        schema::JobserverJob jobTable;
        schema::JobserverJobhistory jobHistoryTable;
        schema::JobserverClusteruuid clusterUuidTable;
        database->run(remove_from(fileDownloadTable).unconditionally());
        database->run(remove_from(jobHistoryTable).unconditionally());
        database->run(remove_from(jobTable).unconditionally());
        database->run(remove_from(clusterUuidTable).unconditionally());

        // Set up the cluster manager
        setenv(CLUSTER_CONFIG_ENV_VARIABLE, base64Encode(sClusters).c_str(), 1);
        auto mgr = std::make_shared<ClusterManager>();

        // Start the cluster scheduler
        bool running = true;
        std::thread clusterThread([&mgr, &running]() {
            while (running) {
                mgr->getvClusters()->at(0)->callrun();
            }
        });

        // Set up the test http server
        setenv(ACCESS_SECRET_ENV_VARIABLE, base64Encode(sAccess).c_str(), 1);
        auto httpSvr = std::make_shared<HttpServer>(mgr);
        httpSvr->start();

        // Set up the test websocket server
        auto wsSrv = WebSocketServer(mgr);
        wsSrv.start();

        BOOST_CHECK_EQUAL(acceptingConnections(8000), true);
        BOOST_CHECK_EQUAL(acceptingConnections(8001), true);

        // Try to reconnect the clusters so that we can get a connection token to use later to connect the client
        mgr->callreconnectClusters();

        // Connect a fake client to the websocket server
        TestWsClient websocketClient("localhost:8001/job/ws/?token=" + getLastToken());
        bool bPaused = false;
        bool bReady = false;
        int pauseCount = 0;
        int resumeCount = 0;
        uint64_t fileSize = 0;
        std::shared_ptr<std::thread> pThread;
        websocketClient.on_message = [&pThread, &bPaused, &bReady, &mgr, &fileSize, &pauseCount, &resumeCount](const std::shared_ptr<TestWsClient::Connection>& connection, const std::shared_ptr<TestWsClient::InMessage>& in_message) {
            auto data = in_message->string();
            auto msg = Message(std::vector<uint8_t>(data.begin(), data.end()));

            // Ignore the ready message
            if (msg.getId() == SERVER_READY) {
                bReady = true;
                return;
            }

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

            msg.pop_uint();
            auto uuid = msg.pop_string();
            auto sBundle = msg.pop_string();
            auto sFilePath = msg.pop_string();

            // Generate a file size between 512 and 1024Mb
            // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
            fileSize = randomInt(1024ULL*1024ULL*512ULL, 1024ULL*1024ULL*1024ULL);

            // Send the file size to the server
            msg = Message(FILE_DETAILS, Message::Priority::Highest, "");
            msg.push_string(uuid);
            msg.push_ulong(fileSize);

            auto smsg = Message(**msg.getdata());
            mgr->getvClusters()->at(0)->callhandleMessage(smsg);

            // Now send the file content in to chunks and send it to the client
            pThread = std::make_shared<std::thread>([&bPaused, connection, fileSize, uuid, &mgr]() {
                // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
                auto CHUNK_SIZE = 1024*64;

                auto data = std::vector<uint8_t>();

                uint64_t bytesSent = 0;
                while (bytesSent < fileSize) {
                    // Spin while the stream is paused
                    while (bPaused) {
                        std::this_thread::yield();
                    }

                    auto chunkSize = std::min(static_cast<uint64_t>(CHUNK_SIZE), static_cast<uint64_t>(fileSize-bytesSent));
                    bytesSent += chunkSize;
                    data.resize(chunkSize);

                    auto msg = Message(FILE_CHUNK, Message::Priority::Lowest, "");
                    msg.push_string(uuid);
                    msg.push_bytes(data);

                    auto smsg = Message(**msg.getdata());
                    mgr->getvClusters()->at(0)->callhandleMessage(smsg);
                }
            });

        };

        // Start the client
        std::thread clientThread([&websocketClient]() {
            websocketClient.start();
        });

        // Wait for the client to connect
        while (!bReady) {
            // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
            std::this_thread::sleep_for(std::chrono::milliseconds(10));
        }

        // Create a job to request files for
        auto jobId = database->run(
                insert_into(jobTable)
                        .set(
                                jobTable.user = 1,
                                jobTable.parameters = "params1",
                                jobTable.cluster = httpSvr->getvJwtSecrets()->at(0).clusters()[0],
                                jobTable.bundle = "whatever",
                                jobTable.application = httpSvr->getvJwtSecrets()->at(0).name()
                        )
        );

        // Create a file download
        // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
        auto now = std::chrono::system_clock::now() + std::chrono::minutes{10};
        jwt::jwt_object jwtToken = {
                jwt::params::algorithm("HS256"),
                jwt::params::payload({{"userName", "User"}}),
                jwt::params::secret(httpSvr->getvJwtSecrets()->at(0).secret())
        };
        jwtToken.add_claim("exp", now);

        // Since payload above only accepts string values, we need to set up any non-string values
        // separately
        // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
        jwtToken.payload().add_claim("userId", 5);

        // Create params
        nlohmann::json params = {
                {"jobId", jobId},
                {"path",  "/data/myfile.png"}
        };

        TestHttpClient httpClient("localhost:8000");
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

        // Finished with the servers and clients
        running = false;
        *mgr->getvClusters()->at(0)->getdataReady() = true;
        mgr->getvClusters()->at(0)->getdataCV()->notify_one();
        clusterThread.join();
        websocketClient.stop();
        clientThread.join();
        httpSvr->stop();
        wsSrv.stop();

        pThread->join();

        // Clean up
        database->run(remove_from(fileDownloadTable).unconditionally());
        database->run(remove_from(jobHistoryTable).unconditionally());
        database->run(remove_from(jobTable).unconditionally());

        std::cout << "Large file test complete. File size was " << fileSize << ". Pauses: " << pauseCount << ", resumes: " << resumeCount << std::endl;
        BOOST_CHECK_EQUAL(pauseCount > 0, true);
        BOOST_CHECK_EQUAL(pauseCount, resumeCount);
    }
BOOST_AUTO_TEST_SUITE_END()

// NOLINTEND(concurrency-mt-unsafe)