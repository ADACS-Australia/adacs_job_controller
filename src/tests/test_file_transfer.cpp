//
// Created by lewis on 10/6/21.
//

#include <boost/test/unit_test.hpp>
#include <nlohmann/json.hpp>
#include <thread>
#include <chrono>
#include <client_ws.hpp>
#include <client_http.hpp>
#include <jwt/jwt.hpp>
#include "../Settings.h"
#include "../Lib/GeneralUtils.h"
#include "../Cluster/ClusterManager.h"
#include "../HTTP/HttpServer.h"
#include "../Lib/jobserver_schema.h"
#include "../DB/MySqlConnector.h"

extern uint32_t randomInt(uint32_t start, uint32_t end);
extern std::vector<uint8_t> *generateRandomData(uint32_t count);

using WsClient = SimpleWeb::SocketClient<SimpleWeb::WS>;
using HttpClient = SimpleWeb::Client<SimpleWeb::HTTP>;

std::string getLastToken() {
    auto db = MySqlConnector();
    schema::JobserverClusteruuid clusterUuidTable;

    // Look up all cluster tokens
    auto uuidResults = db->run(
            select(all_of(clusterUuidTable))
                    .from(clusterUuidTable)
                    .unconditionally()
                    .order_by(clusterUuidTable.id.desc())
    );

    // Check that the uuid was valid
    if (uuidResults.empty())
        throw std::runtime_error("Couldn't get any cluster token?");

    // Return the uuid
    return uuidResults.front().uuid;
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
        auto mgr = ClusterManager();

        // Start the cluster scheduler
        bool running = true;
        std::thread clusterThread([&mgr, &running]() {
            while (running)
                mgr.getvClusters()->at(0)->callrun();
        });

        // Set up the test http server
        setenv(ACCESS_SECRET_ENV_VARIABLE, base64Encode(sAccess).c_str(), 1);
        auto httpSvr = HttpServer(&mgr);
        httpSvr.start();

        // Set up the test websocket server
        auto wsSrv = WebSocketServer(&mgr);
        wsSrv.start();

        std::this_thread::sleep_for(std::chrono::milliseconds(100));

        // Try to reconnect the clusters so that we can get a connection token to use later to connect the client
        mgr.callreconnectClusters();

        // Connect a fake client to the websocket server
        WsClient websocketClient("localhost:8001/job/ws/?token=" + getLastToken());
        auto fileData = new std::vector<uint8_t>();
        websocketClient.on_message = [fileData](const std::shared_ptr<WsClient::Connection>& connection, const std::shared_ptr<WsClient::InMessage>& in_message) {
            auto data = in_message->string();
            auto msg = Message(std::vector<uint8_t>(data.begin(), data.end()));

            // Ignore the ready message
            if (msg.getId() == SERVER_READY)
                return;

            bool bPaused = false;
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

            auto o = std::make_shared<WsClient::OutMessage>(msg.getdata()->size());
            std::ostream_iterator<uint8_t> iter(*o);
            std::copy(msg.getdata()->begin(), msg.getdata()->end(), iter);
            connection->send(o, nullptr, 130);

            // Now send the file content in to chunks and send it to the client
            new std::thread([&bPaused, connection, fileSize, uuid, fileData]() {
                auto CHUNK_SIZE = 1024*64;

                uint64_t bytesSent = 0;
                while (bytesSent < fileSize) {
                    // Don't do anything while the stream is paused
                    while (bPaused)
                        std::this_thread::sleep_for(std::chrono::milliseconds (1));

                    auto chunkSize = std::min((uint32_t) CHUNK_SIZE, (uint32_t) (fileSize-bytesSent));
                    bytesSent += chunkSize;

                    auto data = generateRandomData(chunkSize);

                    fileData->insert(fileData->end(), data->begin(), data->end());

                    auto msg = Message(FILE_CHUNK, Message::Priority::Lowest, "");
                    msg.push_string(uuid);
                    msg.push_bytes(*data);

                    auto o = std::make_shared<WsClient::OutMessage>(msg.getdata()->size());
                    std::ostream_iterator<uint8_t> iter(*o);
                    std::copy(msg.getdata()->begin(), msg.getdata()->end(), iter);
                    connection->send(o, nullptr, 130);

                    delete data;
                }
            });

        };

        // Start the client
        std::thread clientThread([&websocketClient]() {
            websocketClient.start();
        });

        std::this_thread::sleep_for(std::chrono::milliseconds (100));

        // Create a job to request files for
        auto jobId = db->run(
                insert_into(jobTable)
                        .set(
                                jobTable.user = 1,
                                jobTable.parameters = "params1",
                                jobTable.cluster = httpSvr.getvJwtSecrets()->at(0).clusters()[0],
                                jobTable.bundle = "whatever",
                                jobTable.application = httpSvr.getvJwtSecrets()->at(0).name()
                        )
        );

        // Create a file download
        auto now = std::chrono::system_clock::now() + std::chrono::minutes{10};
        jwt::jwt_object jwtToken = {
                jwt::params::algorithm("HS256"),
                jwt::params::payload({{"userName", "User"}}),
                jwt::params::secret(httpSvr.getvJwtSecrets()->at(0).secret())
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

        HttpClient httpClient("localhost:8000");
        auto r = httpClient.request("POST", "/job/apiv1/file/", params.dump(), {{"Authorization", jwtToken.signature()}});

        std::cout << "Content: " << r->content.string() << std::endl;

        nlohmann::json result;
        r->content >> result;

        for (int i = 0; i < 5; i++) {
            // Try to download the file
            r = httpClient.request("GET", "/job/apiv1/file/?fileId=" + std::string(result["fileId"]));
            auto content = r->content.string();

            auto returnData = std::vector<uint8_t>(content.begin(), content.end());

            // Check that the data collected by the client was correct
            BOOST_CHECK_EQUAL_COLLECTIONS(returnData.begin(), returnData.end(), fileData->begin(), fileData->end());
            std::cout << i << std::endl;

            fileData->clear();
        }
        // Finished with the servers and clients
        running = false;
        *mgr.getvClusters()->at(0)->getdataReady() = true;
        mgr.getvClusters()->at(0)->getdataCV()->notify_one();
        clusterThread.join();
        websocketClient.stop();
        clientThread.join();
        httpSvr.stop();
        wsSrv.stop();

        // Clean up
        db->run(remove_from(fileDownloadTable).unconditionally());
        db->run(remove_from(jobHistoryTable).unconditionally());
        db->run(remove_from(jobTable).unconditionally());
    }
}