//
// Created by lewis on 10/6/21.
//

#include <boost/test/unit_test.hpp>
#include <nlohmann/json.hpp>
#include <thread>
#include <chrono>
#include <jwt/jwt.hpp>
#include "../Settings.h"
#include "../Lib/GeneralUtils.h"
#include "../Cluster/ClusterManager.h"
#include "utils.h"

size_t parseLine(char* line){
    // This assumes that a digit will be found and the line ends in " Kb".
    size_t i = strlen(line);
    const char* p = line;
    while (*p <'0' || *p > '9') p++;
    line[i-3] = '\0';
    i = std::atol(p);
    return i;
}

size_t getCurrentMemoryUsage() {
    FILE* file = fopen("/proc/self/status", "r");
    size_t result = -1;
    char line[128];

    while (fgets(line, 128, file) != nullptr){
        if (strncmp(line, "VmRSS:", 6) == 0){
            result = parseLine(line);
            break;
        }
    }
    fclose(file);
    return result;
}

BOOST_AUTO_TEST_SUITE(file_transfer_test_suite)
    auto sAccess = R"(
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

    auto sClusters = R"(
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
        auto db = MySqlConnector();
        schema::JobserverFiledownload fileDownloadTable;
        schema::JobserverJob jobTable;
        schema::JobserverJobhistory jobHistoryTable;
        schema::JobserverClusteruuid clusterUuidTable;
        db->run(remove_from(fileDownloadTable).unconditionally());
        db->run(remove_from(jobHistoryTable).unconditionally());
        db->run(remove_from(jobTable).unconditionally());
        db->run(remove_from(clusterUuidTable).unconditionally());

        // Set up the cluster manager
        setenv(CLUSTER_CONFIG_ENV_VARIABLE, base64Encode(sClusters).c_str(), 1);
        auto mgr = std::make_shared<ClusterManager>();

        // Start the cluster scheduler
        bool running = true;
        std::thread clusterThread([&mgr, &running]() {
            while (running)
                mgr->getvClusters()->at(0)->callrun();
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
        auto fileData = new std::vector<uint8_t>();
        bool* bPaused = new bool;
        *bPaused = false;
        std::thread* pThread;
        websocketClient.on_message = [&pThread, fileData, bPaused](const std::shared_ptr<TestWsClient::Connection>& connection, const std::shared_ptr<TestWsClient::InMessage>& in_message) {
            auto data = in_message->string();
            auto msg = Message(std::vector<uint8_t>(data.begin(), data.end()));

            // Ignore the ready message
            if (msg.getId() == SERVER_READY)
                return;

            // Check if this is a pause transfer message
            if (msg.getId() == PAUSE_FILE_CHUNK_STREAM) {
                *bPaused = true;
                return;
            }

            // Check if this is a resume transfer message
            if (msg.getId() == RESUME_FILE_CHUNK_STREAM) {
                *bPaused = false;
                return;
            }

            auto jobId = msg.pop_uint();
            auto uuid = msg.pop_string();
            auto sBundle = msg.pop_string();
            auto sFilePath = msg.pop_string();

            auto fileSize = randomInt(0, 1024*1024);
            fileData->reserve(fileSize);

            // Send the file size to the server
            msg = Message(FILE_DETAILS, Message::Priority::Highest, "");
            msg.push_string(uuid);
            msg.push_ulong(fileSize);

            auto o = std::make_shared<TestWsClient::OutMessage>(msg.getdata()->get()->size());
            std::ostream_iterator<uint8_t> iter(*o);
            std::copy(msg.getdata()->get()->begin(), msg.getdata()->get()->end(), iter);
            connection->send(o, nullptr, 130);

            // Now send the file content in to chunks and send it to the client
            pThread = new std::thread([bPaused, connection, fileSize, uuid, fileData]() {
                auto CHUNK_SIZE = 1024*64;

                uint64_t bytesSent = 0;
                while (bytesSent < fileSize) {
                    // Don't do anything while the stream is paused
                    while (*bPaused)
                        std::this_thread::sleep_for(std::chrono::milliseconds (1));

                    auto chunkSize = std::min((uint32_t) CHUNK_SIZE, (uint32_t) (fileSize-bytesSent));
                    bytesSent += chunkSize;

                    auto data = generateRandomData(chunkSize);

                    fileData->insert(fileData->end(), data->begin(), data->end());

                    auto msg = Message(FILE_CHUNK, Message::Priority::Lowest, "");
                    msg.push_string(uuid);
                    msg.push_bytes(*data);

                    auto o = std::make_shared<TestWsClient::OutMessage>(msg.getdata()->get()->size());
                    std::ostream_iterator<uint8_t> iter(*o);
                    std::copy(msg.getdata()->get()->begin(), msg.getdata()->get()->end(), iter);
                    connection->send(o, nullptr, 130);
                }
            });

        };

        // Start the client
        std::thread clientThread([&websocketClient]() {
            websocketClient.start();
        });

        std::this_thread::sleep_for(std::chrono::milliseconds(100));

        // Create a job to request files for
        auto jobId = db->run(
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
        auto now = std::chrono::system_clock::now() + std::chrono::minutes{10};
        jwt::jwt_object jwtToken = {
                jwt::params::algorithm("HS256"),
                jwt::params::payload({{"userName", "User"}}),
                jwt::params::secret(httpSvr->getvJwtSecrets()->at(0).secret())
        };
        jwtToken.add_claim("exp", now);

        // Since payload above only accepts string values, we need to set up any non-string values
        // separately
        jwtToken.payload().add_claim("userId", 5);

        // Create params
        nlohmann::json params = {
                {"jobId", jobId},
                {"path",  "/data/myfile.png"}
        };

        TestHttpClient httpClient("localhost:8000");
        auto r = httpClient.request("POST", "/job/apiv1/file/", params.dump(), {{"Authorization", jwtToken.signature()}});

        nlohmann::json result;
        r->content >> result;

        for (int i = 0; i < 5; i++) {
            // Try to download the file
            r = httpClient.request("GET", "/job/apiv1/file/?fileId=" + std::string(result["fileId"]));
            auto content = r->content.string();

            auto returnData = std::vector<uint8_t>(content.begin(), content.end());

            // Check that the data collected by the client was correct
            BOOST_CHECK_EQUAL_COLLECTIONS(returnData.begin(), returnData.end(), fileData->begin(), fileData->end());

            fileData->clear();

            pThread->join();
            delete pThread;
            pThread = nullptr;
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

        delete bPaused;
        delete fileData;

        // Clean up
        db->run(remove_from(fileDownloadTable).unconditionally());
        db->run(remove_from(jobHistoryTable).unconditionally());
        db->run(remove_from(jobTable).unconditionally());
    }

    BOOST_AUTO_TEST_CASE(test_large_file_transfers) {
        // Delete all database info just in case
        auto db = MySqlConnector();
        schema::JobserverFiledownload fileDownloadTable;
        schema::JobserverJob jobTable;
        schema::JobserverJobhistory jobHistoryTable;
        schema::JobserverClusteruuid clusterUuidTable;
        db->run(remove_from(fileDownloadTable).unconditionally());
        db->run(remove_from(jobHistoryTable).unconditionally());
        db->run(remove_from(jobTable).unconditionally());
        db->run(remove_from(clusterUuidTable).unconditionally());

        // Set up the cluster manager
        setenv(CLUSTER_CONFIG_ENV_VARIABLE, base64Encode(sClusters).c_str(), 1);
        auto mgr = std::make_shared<ClusterManager>();

        // Start the cluster scheduler
        bool running = true;
        std::thread clusterThread([&mgr, &running]() {
            while (running)
                mgr->getvClusters()->at(0)->callrun();
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
        bool* bPaused = new bool;
        bool* bReady = new bool;
        *bPaused = false;
        uint64_t fileSize;
        std::thread* pThread;
        websocketClient.on_message = [&pThread, bPaused, bReady, &mgr, &fileSize](const std::shared_ptr<TestWsClient::Connection>& connection, const std::shared_ptr<TestWsClient::InMessage>& in_message) {
            auto data = in_message->string();
            auto msg = Message(std::vector<uint8_t>(data.begin(), data.end()));

            // Ignore the ready message
            if (msg.getId() == SERVER_READY) {
                *bReady = true;
                return;
            }

            // Check if this is a pause transfer message
            if (msg.getId() == PAUSE_FILE_CHUNK_STREAM) {
                *bPaused = true;
                return;
            }

            // Check if this is a resume transfer message
            if (msg.getId() == RESUME_FILE_CHUNK_STREAM) {
                *bPaused = false;
                return;
            }

            auto jobId = msg.pop_uint();
            auto uuid = msg.pop_string();
            auto sBundle = msg.pop_string();
            auto sFilePath = msg.pop_string();

            // Generate a file size between 512 and 1024Mb
            fileSize = randomInt(1024ull*1024ull*512ull, 1024ull*1024ull*1024ull);

            // Send the file size to the server
            msg = Message(FILE_DETAILS, Message::Priority::Highest, "");
            msg.push_string(uuid);
            msg.push_ulong(fileSize);

            auto smsg = Message(*msg.getdata()->get());
            mgr->getvClusters()->at(0)->callhandleMessage(smsg);

            // Now send the file content in to chunks and send it to the client
            pThread = new std::thread([bPaused, connection, fileSize, uuid, &mgr]() {
                auto CHUNK_SIZE = 1024*64;

                auto data = std::vector<uint8_t>();

                uint64_t bytesSent = 0;
                while (bytesSent < fileSize) {
                    // Don't do anything while the stream is paused
                    while (*bPaused)
                        std::this_thread::sleep_for(std::chrono::milliseconds (1));

                    auto chunkSize = std::min((uint32_t) CHUNK_SIZE, (uint32_t) (fileSize-bytesSent));
                    bytesSent += chunkSize;
                    data.resize(chunkSize);

                    auto msg = Message(FILE_CHUNK, Message::Priority::Lowest, "");
                    msg.push_string(uuid);
                    msg.push_bytes(data);

                    auto smsg = Message(*msg.getdata()->get());
                    mgr->getvClusters()->at(0)->callhandleMessage(smsg);
                }
            });

        };

        // Start the client
        std::thread clientThread([&websocketClient]() {
            websocketClient.start();
        });

        // Wait for the client to connect
        while (!*bReady) {
            std::this_thread::sleep_for(std::chrono::milliseconds(10));
        }

        // Create a job to request files for
        auto jobId = db->run(
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
        auto now = std::chrono::system_clock::now() + std::chrono::minutes{10};
        jwt::jwt_object jwtToken = {
                jwt::params::algorithm("HS256"),
                jwt::params::payload({{"userName", "User"}}),
                jwt::params::secret(httpSvr->getvJwtSecrets()->at(0).secret())
        };
        jwtToken.add_claim("exp", now);

        // Since payload above only accepts string values, we need to set up any non-string values
        // separately
        jwtToken.payload().add_claim("userId", 5);

        // Create params
        nlohmann::json params = {
                {"jobId", jobId},
                {"path",  "/data/myfile.png"}
        };

        TestHttpClient httpClient("localhost:8000");
        auto r = httpClient.request("POST", "/job/apiv1/file/", params.dump(), {{"Authorization", jwtToken.signature()}});

        nlohmann::json result;
        r->content >> result;

        // Try to download the file
        uint64_t totalBytesReceived = 0;
        bool end = false;
        httpClient.config.max_response_streambuf_size = 1024*1024;
        httpClient.request(
            "GET",
            "/job/apiv1/file/?fileId=" + std::string(result["fileId"]),
            [&totalBytesReceived, &end](const std::shared_ptr<TestHttpClient::Response>& response, const SimpleWeb::error_code &ec) {
                totalBytesReceived += response->content.size();
                end = response->content.end;
            }
        );

        // While the file download hasn't finished, check that the memory usage never exceeds 200Mb
        long long baselineMemUsage = getCurrentMemoryUsage();
        while (!end) {
            auto memUsage = (long long) getCurrentMemoryUsage();
            if (baselineMemUsage - memUsage > 1024*200)
                BOOST_ASSERT_MSG(false, "Maximum tolerable memory usage was exceeded");

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
        delete pThread;

        delete bPaused;

        // Clean up
        db->run(remove_from(fileDownloadTable).unconditionally());
        db->run(remove_from(jobHistoryTable).unconditionally());
        db->run(remove_from(jobTable).unconditionally());
    }
BOOST_AUTO_TEST_SUITE_END();