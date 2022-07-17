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
#include "../HTTP/Utils/HandleFileList.h"
#include <boost/filesystem/path.hpp>

extern uint64_t randomInt(uint64_t start, uint64_t end);

using WsClient = SimpleWeb::SocketClient<SimpleWeb::WS>;
using HttpClient = SimpleWeb::Client<SimpleWeb::HTTP>;

extern std::string getLastToken();

BOOST_AUTO_TEST_SUITE(file_list_caching_test_suite)
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

    // Create a file list
    std::vector<sFile> fileListData = {
            {"/", 0, 0, true},
            {"/test", randomInt(0, (uint64_t) -1), 0, false},
            {"/testdir", 0, 0, true},
            {"/testdir/file", randomInt(0, (uint64_t) -1), 0, false},
            {"/testdir/file2", randomInt(0, (uint64_t) -1), 0, false},
            {"/testdir/file3", randomInt(0, (uint64_t) -1), 0, false},
            {"/testdir/testdir1", 0, 0, true},
            {"/testdir/testdir1/file", randomInt(0, (uint64_t) -1), 0, false},
            {"/test2", randomInt(0, (uint64_t) -1), 0, false},
    };

    BOOST_AUTO_TEST_CASE(test_file_list) {
        // Delete all database info just in case
        auto db = MySqlConnector();
        schema::JobserverFiledownload fileDownloadTable;
        schema::JobserverFilelistcache fileListCacheTable;
        schema::JobserverJob jobTable;
        schema::JobserverJobhistory jobHistoryTable;
        schema::JobserverClusteruuid clusterUuidTable;
        db->run(remove_from(fileListCacheTable).unconditionally());
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
        auto httpSvr = HttpServer(mgr);
        httpSvr.start();

        // Set up the test websocket server
        auto wsSrv = WebSocketServer(mgr);
        wsSrv.start();

        BOOST_CHECK_EQUAL(acceptingConnections(8000), true);
        BOOST_CHECK_EQUAL(acceptingConnections(8001), true);

        // Try to reconnect the clusters so that we can get a connection token to use later to connect the client
        mgr->callreconnectClusters();

        // Connect a fake client to the websocket server
        bool bRaiseError = true;
        std::vector<std::string> lastDirPath;
        std::vector<bool> lastbRecursive;
        WsClient websocketClient("localhost:8001/job/ws/?token=" + getLastToken());
        websocketClient.on_message = [&lastDirPath, &lastbRecursive, &bRaiseError](const std::shared_ptr<WsClient::Connection>& connection, const std::shared_ptr<WsClient::InMessage>& in_message) {
            auto data = in_message->string();
            auto msg = Message(std::vector<uint8_t>(data.begin(), data.end()));

            // Ignore the ready message
            if (msg.getId() == SERVER_READY)
                return;

            // Check that this message can only be a FILE_LIST request
            if (msg.getId() != FILE_LIST) {
                BOOST_ASSERT_MSG(false, "Got unexpected websocket message");
                return;
            }

            auto jobId = msg.pop_uint();
            auto uuid = msg.pop_string();
            auto bundleHash = msg.pop_string();

            // Check if we should raise an error
            if (bRaiseError) {
                msg = Message(FILE_LIST_ERROR, Message::Priority::Highest, "");
                msg.push_string(uuid);
                msg.push_string("test error");

                auto o = std::make_shared<WsClient::OutMessage>(msg.getdata()->get()->size());
                std::ostream_iterator<uint8_t> iter(*o);
                std::copy(msg.getdata()->get()->begin(), msg.getdata()->get()->end(), iter);
                connection->send(o, nullptr, 130);
                return;
            }

            lastDirPath.push_back(msg.pop_string());
            lastbRecursive.push_back(msg.pop_bool());

            auto filteredFiles = filterFiles(fileListData, lastDirPath.back(), lastbRecursive.back());

            msg = Message(FILE_LIST, Message::Priority::Highest, "");
            msg.push_string(uuid);
            msg.push_uint(filteredFiles.size());

            for (auto &f : filteredFiles) {
                auto p = boost::filesystem::path(f.fileName);

                msg.push_string(f.fileName);
                msg.push_bool(f.isDirectory);
                msg.push_ulong(f.fileSize);
            }

            auto o = std::make_shared<WsClient::OutMessage>(msg.getdata()->get()->size());
            std::ostream_iterator<uint8_t> iter(*o);
            std::copy(msg.getdata()->get()->begin(), msg.getdata()->get()->end(), iter);
            connection->send(o, nullptr, 130);
        };

        // Start the client
        std::thread clientThread([&websocketClient]() {
            websocketClient.start();
        });

        std::this_thread::sleep_for(std::chrono::milliseconds(100));

        // Create a job to request file lists for
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

        // Set up the auth token
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
                {"recursive", false},
                {"path",  "/testdir"}
        };

        // This call should cause an error to be raised and reported
        HttpClient httpClient("localhost:8000");
        auto r = httpClient.request("PATCH", "/job/apiv1/file/", params.dump(), {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(std::stoi(r->status_code), (int) SimpleWeb::StatusCode::client_error_bad_request);
        BOOST_CHECK_EQUAL(r->content.string(), "test error");
        BOOST_CHECK_EQUAL(fileListMap->size(), 0);

        // Finish error testing
        bRaiseError = false;

        // This file list will be complete, but won't create any file cache objects because the job has no
        // _job_completion_ job history objects
        r = httpClient.request("PATCH", "/job/apiv1/file/", params.dump(), {{"Authorization", jwtToken.signature()}});

        BOOST_CHECK_EQUAL(fileListMap->size(), 0);

        BOOST_CHECK_EQUAL(lastDirPath.size(), 1);
        BOOST_CHECK_EQUAL(lastDirPath.back(), std::string(params["path"]));
        BOOST_CHECK_EQUAL(lastbRecursive.back(), (bool) params["recursive"]);

        std::vector<sFile> expected = {
                fileListData[2],
                fileListData[3],
                fileListData[4],
                fileListData[5],
                fileListData[6]
        };

        nlohmann::json result;
        r->content >> result;
        BOOST_CHECK(result.find("files") != result.end());
        auto jsonData = result["files"];

        // Check that the file list returned was correct
        for (auto i = 0; i < expected.size(); i++) {
            auto f = expected[i];

            BOOST_CHECK_EQUAL(jsonData[i]["path"], f.fileName);
            BOOST_CHECK_EQUAL(jsonData[i]["isDir"], f.isDirectory);
            BOOST_CHECK_EQUAL(jsonData[i]["fileSize"], f.fileSize);
            BOOST_CHECK_EQUAL(jsonData[i]["permissions"], f.permissions);
        }

        // Next we want to create a _job_completion_ file history object and request files from a sub directory
        db->run(
                insert_into(jobHistoryTable)
                        .set(
                                jobHistoryTable.jobId = jobId,
                                jobHistoryTable.timestamp = std::chrono::system_clock::now(),
                                jobHistoryTable.what = "_job_completion_",
                                jobHistoryTable.state = 500,
                                jobHistoryTable.details = "Job is complete"
                        )
        );

        // Reset the websocket trackers
        lastDirPath.clear();
        lastbRecursive.clear();

        // This file list will be complete, and should also create the file list cache since now a _job_completion_ job
        // history record exists.
        r = httpClient.request("PATCH", "/job/apiv1/file/", params.dump(), {{"Authorization", jwtToken.signature()}});

        // There should have been two websocket calls - one to get the file list of the initial request, and a second
        // to get the full recursive file list of the entire job
        BOOST_CHECK_EQUAL(lastDirPath.size(), 2);
        BOOST_CHECK_EQUAL(lastDirPath[0], std::string(params["path"]));
        BOOST_CHECK_EQUAL(lastDirPath[1], "");
        BOOST_CHECK_EQUAL(lastbRecursive[0], (bool) params["recursive"]);
        BOOST_CHECK_EQUAL(lastbRecursive[1], true);

        r->content >> result;
        BOOST_CHECK(result.find("files") != result.end());
        jsonData = result["files"];

        // Check that the file list returned was correct
        for (auto i = 0; i < expected.size(); i++) {
            auto f = expected[i];

            BOOST_CHECK_EQUAL(jsonData[i]["path"], f.fileName);
            BOOST_CHECK_EQUAL(jsonData[i]["isDir"], f.isDirectory);
            BOOST_CHECK_EQUAL(jsonData[i]["fileSize"], f.fileSize);
            BOOST_CHECK_EQUAL(jsonData[i]["permissions"], f.permissions);
        }

        // Check that the correct file list cache entries were added to the database
        auto fileListCacheResult = db->run(
                select(all_of(fileListCacheTable))
                        .from(fileListCacheTable)
                        .where(
                                fileListCacheTable.jobId == (uint32_t) jobId
                        )
        );

        BOOST_CHECK_EQUAL(fileListCacheResult.empty(), false);

        std::vector<sFile> files;
        for (auto &f : fileListCacheResult) {
            files.push_back({
                                    f.path,
                                    (uint64_t) f.fileSize,
                                    (uint32_t) f.permissions,
                                    (bool) f.isDir
                            });
        }

        for (auto i = 0; i < files.size(); i++) {
            BOOST_CHECK_EQUAL(files[i].fileName, fileListData[i].fileName);
            BOOST_CHECK_EQUAL(files[i].isDirectory, fileListData[i].isDirectory);
            BOOST_CHECK_EQUAL(files[i].fileSize, fileListData[i].fileSize);
            BOOST_CHECK_EQUAL(files[i].permissions, fileListData[i].permissions);
        }

        // Reset the websocket trackers
        lastDirPath.clear();
        lastbRecursive.clear();

        // This file list will be complete, and should not call the websocket since all files should be cached
        r = httpClient.request("PATCH", "/job/apiv1/file/", params.dump(), {{"Authorization", jwtToken.signature()}});

        // There should have been no websocket calls
        BOOST_CHECK_EQUAL(lastDirPath.size(), 0);

        r->content >> result;
        BOOST_CHECK(result.find("files") != result.end());
        jsonData = result["files"];

        // Check that the file list returned was correct
        for (auto i = 0; i < expected.size(); i++) {
            auto f = expected[i];

            BOOST_CHECK_EQUAL(jsonData[i]["path"], f.fileName);
            BOOST_CHECK_EQUAL(jsonData[i]["isDir"], f.isDirectory);
            BOOST_CHECK_EQUAL(jsonData[i]["fileSize"], f.fileSize);
            BOOST_CHECK_EQUAL(jsonData[i]["permissions"], f.permissions);
        }

        params = {
                {"jobId", jobId},
                {"recursive", true},
                {"path",  "/"}
        };

        // This file list will be complete, and should not call the websocket since all files should be cached
        r = httpClient.request("PATCH", "/job/apiv1/file/", params.dump(), {{"Authorization", jwtToken.signature()}});

        // There should have been no websocket calls
        BOOST_CHECK_EQUAL(lastDirPath.size(), 0);

        r->content >> result;
        BOOST_CHECK(result.find("files") != result.end());
        jsonData = result["files"];

        // Check that the file list returned was correct
        for (auto i = 0; i < fileListData.size(); i++) {
            auto f = fileListData[i];

            BOOST_CHECK_EQUAL(jsonData[i]["path"], f.fileName);
            BOOST_CHECK_EQUAL(jsonData[i]["isDir"], f.isDirectory);
            BOOST_CHECK_EQUAL(jsonData[i]["fileSize"], f.fileSize);
            BOOST_CHECK_EQUAL(jsonData[i]["permissions"], f.permissions);
        }

        params = {
                {"jobId", jobId},
                {"recursive", true},
                {"path",  "/testdir"}
        };

        // This file list will be complete, and should not call the websocket since all files should be cached
        r = httpClient.request("PATCH", "/job/apiv1/file/", params.dump(), {{"Authorization", jwtToken.signature()}});

        // There should have been no websocket calls
        BOOST_CHECK_EQUAL(lastDirPath.size(), 0);

        r->content >> result;
        BOOST_CHECK(result.find("files") != result.end());
        jsonData = result["files"];

        expected = {
                fileListData[2],
                fileListData[3],
                fileListData[4],
                fileListData[5],
                fileListData[6],
                fileListData[7]
        };

        // Check that the file list returned was correct
        for (auto i = 0; i < expected.size(); i++) {
            auto f = expected[i];

            BOOST_CHECK_EQUAL(jsonData[i]["path"], f.fileName);
            BOOST_CHECK_EQUAL(jsonData[i]["isDir"], f.isDirectory);
            BOOST_CHECK_EQUAL(jsonData[i]["fileSize"], f.fileSize);
            BOOST_CHECK_EQUAL(jsonData[i]["permissions"], f.permissions);
        }

        // Finished with the servers and clients
        running = false;
        *mgr->getvClusters()->at(0)->getdataReady() = true;
        mgr->getvClusters()->at(0)->getdataCV()->notify_one();
        clusterThread.join();
        websocketClient.stop();
        clientThread.join();
        httpSvr.stop();
        wsSrv.stop();

        // Clean up
        db->run(remove_from(fileListCacheTable).unconditionally());
        db->run(remove_from(jobHistoryTable).unconditionally());
        db->run(remove_from(jobTable).unconditionally());
    }

    BOOST_AUTO_TEST_CASE(test_job_finished_update) {
        // Delete all database info just in case
        auto db = MySqlConnector();
        schema::JobserverFiledownload fileDownloadTable;
        schema::JobserverFilelistcache fileListCacheTable;
        schema::JobserverJob jobTable;
        schema::JobserverJobhistory jobHistoryTable;
        schema::JobserverClusteruuid clusterUuidTable;
        db->run(remove_from(fileListCacheTable).unconditionally());
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
        auto httpSvr = HttpServer(mgr);
        httpSvr.start();

        // Set up the test websocket server
        auto wsSrv = WebSocketServer(mgr);
        wsSrv.start();

        BOOST_CHECK_EQUAL(acceptingConnections(8000), true);
        BOOST_CHECK_EQUAL(acceptingConnections(8001), true);

        // Try to reconnect the clusters so that we can get a connection token to use later to connect the client
        mgr->callreconnectClusters();

        // Connect a fake client to the websocket server
        std::vector<std::string> lastDirPath;
        std::vector<bool> lastbRecursive;
        WsClient websocketClient("localhost:8001/job/ws/?token=" + getLastToken());
        websocketClient.on_message = [&lastDirPath, &lastbRecursive](const std::shared_ptr<WsClient::Connection>& connection, const std::shared_ptr<WsClient::InMessage>& in_message) {
            auto data = in_message->string();
            auto msg = Message(std::vector<uint8_t>(data.begin(), data.end()));

            // Ignore the ready message
            if (msg.getId() == SERVER_READY)
                return;

            // Check that this message can only be a FILE_LIST request
            if (msg.getId() != FILE_LIST) {
                BOOST_ASSERT_MSG(false, "Got unexpected websocket message");
                return;
            }

            auto jobId = msg.pop_uint();
            auto uuid = msg.pop_string();
            auto bundleHash = msg.pop_string();
            lastDirPath.push_back(msg.pop_string());
            lastbRecursive.push_back(msg.pop_bool());

            auto filteredFiles = filterFiles(fileListData, lastDirPath.back(), lastbRecursive.back());

            msg = Message(FILE_LIST, Message::Priority::Highest, "");
            msg.push_string(uuid);
            msg.push_uint(filteredFiles.size());

            for (auto &f : filteredFiles) {
                auto p = boost::filesystem::path(f.fileName);

                msg.push_string(f.fileName);
                msg.push_bool(f.isDirectory);
                msg.push_ulong(f.fileSize);
            }

            auto o = std::make_shared<WsClient::OutMessage>(msg.getdata()->get()->size());
            std::ostream_iterator<uint8_t> iter(*o);
            std::copy(msg.getdata()->get()->begin(), msg.getdata()->get()->end(), iter);
            connection->send(o, nullptr, 130);
        };

        // Start the client
        std::thread clientThread([&websocketClient]() {
            websocketClient.start();
        });

        std::this_thread::sleep_for(std::chrono::milliseconds(100));

        // Create a job to request file lists for
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

        // Create a file list
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
                {"recursive", false},
                {"path",  "/testdir"}
        };

        // This file list will be complete, but won't create any file cache objects because the job has no
        // _job_completion_ job history objects
        HttpClient httpClient("localhost:8000");
        auto r = httpClient.request("PATCH", "/job/apiv1/file/", params.dump(), {{"Authorization", jwtToken.signature()}});

        BOOST_CHECK_EQUAL(lastDirPath.size(), 1);
        BOOST_CHECK_EQUAL(lastDirPath.back(), std::string(params["path"]));
        BOOST_CHECK_EQUAL(lastbRecursive.back(), (bool) params["recursive"]);

        std::vector<sFile> expected = {
                fileListData[2],
                fileListData[3],
                fileListData[4],
                fileListData[5],
                fileListData[6]
        };

        nlohmann::json result;
        r->content >> result;
        BOOST_CHECK(result.find("files") != result.end());
        auto jsonData = result["files"];

        // Check that the file list returned was correct
        for (auto i = 0; i < expected.size(); i++) {
            auto f = expected[i];

            BOOST_CHECK_EQUAL(jsonData[i]["path"], f.fileName);
            BOOST_CHECK_EQUAL(jsonData[i]["isDir"], f.isDirectory);
            BOOST_CHECK_EQUAL(jsonData[i]["fileSize"], f.fileSize);
            BOOST_CHECK_EQUAL(jsonData[i]["permissions"], f.permissions);
        }

        // Reset the websocket trackers
        lastDirPath.clear();
        lastbRecursive.clear();

        // Now if we update the job status with a status other than _job_completion_,
        // then all files should still be read from the websocket
        Message msg(UPDATE_JOB);
        msg.push_uint(jobId);                   // jobId
        msg.push_string("running");             // what
        msg.push_uint(200);                     // status
        msg.push_string("it's fine");           // details
        mgr->getvClusters()->at(0)->handleMessage(msg);

        r = httpClient.request("PATCH", "/job/apiv1/file/", params.dump(), {{"Authorization", jwtToken.signature()}});

        BOOST_CHECK_EQUAL(lastDirPath.size(), 1);
        BOOST_CHECK_EQUAL(lastDirPath.back(), std::string(params["path"]));
        BOOST_CHECK_EQUAL(lastbRecursive.back(), (bool) params["recursive"]);

        r->content >> result;
        BOOST_CHECK(result.find("files") != result.end());
        jsonData = result["files"];

        // Check that the file list returned was correct
        for (auto i = 0; i < expected.size(); i++) {
            auto f = expected[i];

            BOOST_CHECK_EQUAL(jsonData[i]["path"], f.fileName);
            BOOST_CHECK_EQUAL(jsonData[i]["isDir"], f.isDirectory);
            BOOST_CHECK_EQUAL(jsonData[i]["fileSize"], f.fileSize);
            BOOST_CHECK_EQUAL(jsonData[i]["permissions"], f.permissions);
        }

        // Reset the websocket trackers
        lastDirPath.clear();
        lastbRecursive.clear();

        // Now if we update the job status with a status for _job_completion_, then all files should be read from
        // the file list cache
        msg = Message(UPDATE_JOB);
        msg.push_uint(jobId);                   // jobId
        msg.push_string("_job_completion_");    // what
        msg.push_uint(400);                     // status
        msg.push_string("it's fine");           // details
        mgr->getvClusters()->at(0)->handleMessage(msg);

        BOOST_CHECK_EQUAL(lastDirPath.size(), 1);
        BOOST_CHECK_EQUAL(lastDirPath.back(), "");
        BOOST_CHECK_EQUAL(lastbRecursive.back(), true);

        // Reset the websocket trackers
        lastDirPath.clear();
        lastbRecursive.clear();

        // This file list will be complete, and should read files from only the file cache
        r = httpClient.request("PATCH", "/job/apiv1/file/", params.dump(), {{"Authorization", jwtToken.signature()}});

        // There should have been two websocket calls - one to get the file list of the initial request, and a second
        // to get the full recursive file list of the entire job
        BOOST_CHECK_EQUAL(lastDirPath.size(), 0);

        r->content >> result;
        BOOST_CHECK(result.find("files") != result.end());
        jsonData = result["files"];

        // Check that the file list returned was correct
        for (auto i = 0; i < expected.size(); i++) {
            auto f = expected[i];

            BOOST_CHECK_EQUAL(jsonData[i]["path"], f.fileName);
            BOOST_CHECK_EQUAL(jsonData[i]["isDir"], f.isDirectory);
            BOOST_CHECK_EQUAL(jsonData[i]["fileSize"], f.fileSize);
            BOOST_CHECK_EQUAL(jsonData[i]["permissions"], f.permissions);
        }

        // Check that the correct file list cache entries were added to the database
        auto fileListCacheResult = db->run(
                select(all_of(fileListCacheTable))
                        .from(fileListCacheTable)
                        .where(
                                fileListCacheTable.jobId == (uint32_t) jobId
                        )
        );

        BOOST_CHECK_EQUAL(fileListCacheResult.empty(), false);

        std::vector<sFile> files;
        for (auto &f : fileListCacheResult) {
            files.push_back({
                                    f.path,
                                    (uint64_t) f.fileSize,
                                    (uint32_t) f.permissions,
                                    (bool) f.isDir
                            });
        }

        for (auto i = 0; i < files.size(); i++) {
            BOOST_CHECK_EQUAL(files[i].fileName, fileListData[i].fileName);
            BOOST_CHECK_EQUAL(files[i].isDirectory, fileListData[i].isDirectory);
            BOOST_CHECK_EQUAL(files[i].fileSize, fileListData[i].fileSize);
            BOOST_CHECK_EQUAL(files[i].permissions, fileListData[i].permissions);
        }

        // Reset the websocket trackers
        lastDirPath.clear();
        lastbRecursive.clear();

        // This file list will be complete, and should not call the websocket since all files should be cached
        r = httpClient.request("PATCH", "/job/apiv1/file/", params.dump(), {{"Authorization", jwtToken.signature()}});

        // There should have been no websocket calls
        BOOST_CHECK_EQUAL(lastDirPath.size(), 0);

        r->content >> result;
        BOOST_CHECK(result.find("files") != result.end());
        jsonData = result["files"];

        // Check that the file list returned was correct
        for (auto i = 0; i < expected.size(); i++) {
            auto f = expected[i];

            BOOST_CHECK_EQUAL(jsonData[i]["path"], f.fileName);
            BOOST_CHECK_EQUAL(jsonData[i]["isDir"], f.isDirectory);
            BOOST_CHECK_EQUAL(jsonData[i]["fileSize"], f.fileSize);
            BOOST_CHECK_EQUAL(jsonData[i]["permissions"], f.permissions);
        }

        params = {
                {"jobId", jobId},
                {"recursive", true},
                {"path",  "/"}
        };

        // This file list will be complete, and should not call the websocket since all files should be cached
        r = httpClient.request("PATCH", "/job/apiv1/file/", params.dump(), {{"Authorization", jwtToken.signature()}});

        // There should have been no websocket calls
        BOOST_CHECK_EQUAL(lastDirPath.size(), 0);

        r->content >> result;
        BOOST_CHECK(result.find("files") != result.end());
        jsonData = result["files"];

        // Check that the file list returned was correct
        for (auto i = 0; i < fileListData.size(); i++) {
            auto f = fileListData[i];

            BOOST_CHECK_EQUAL(jsonData[i]["path"], f.fileName);
            BOOST_CHECK_EQUAL(jsonData[i]["isDir"], f.isDirectory);
            BOOST_CHECK_EQUAL(jsonData[i]["fileSize"], f.fileSize);
            BOOST_CHECK_EQUAL(jsonData[i]["permissions"], f.permissions);
        }

        params = {
                {"jobId", jobId},
                {"recursive", true},
                {"path",  "/testdir"}
        };

        // This file list will be complete, and should not call the websocket since all files should be cached
        r = httpClient.request("PATCH", "/job/apiv1/file/", params.dump(), {{"Authorization", jwtToken.signature()}});

        // There should have been no websocket calls
        BOOST_CHECK_EQUAL(lastDirPath.size(), 0);

        r->content >> result;
        BOOST_CHECK(result.find("files") != result.end());
        jsonData = result["files"];

        expected = {
                fileListData[2],
                fileListData[3],
                fileListData[4],
                fileListData[5],
                fileListData[6],
                fileListData[7]
        };

        // Check that the file list returned was correct
        for (auto i = 0; i < expected.size(); i++) {
            auto f = expected[i];

            BOOST_CHECK_EQUAL(jsonData[i]["path"], f.fileName);
            BOOST_CHECK_EQUAL(jsonData[i]["isDir"], f.isDirectory);
            BOOST_CHECK_EQUAL(jsonData[i]["fileSize"], f.fileSize);
            BOOST_CHECK_EQUAL(jsonData[i]["permissions"], f.permissions);
        }

        // Finished with the servers and clients
        running = false;
        *mgr->getvClusters()->at(0)->getdataReady() = true;
        mgr->getvClusters()->at(0)->getdataCV()->notify_one();
        clusterThread.join();
        websocketClient.stop();
        clientThread.join();
        httpSvr.stop();
        wsSrv.stop();

        // Clean up
        db->run(remove_from(fileListCacheTable).unconditionally());
        db->run(remove_from(jobHistoryTable).unconditionally());
        db->run(remove_from(jobTable).unconditionally());
    }
BOOST_AUTO_TEST_SUITE_END();