//
// Created by lewis on 10/6/21.
//

#include "../Cluster/ClusterManager.h"
#include "../HTTP/Utils/HandleFileList.h"
#include "../Lib/JobStatus.h"
#include "utils.h"
#include <boost/test/unit_test.hpp>
#include <jwt/jwt.hpp>

// NOLINTBEGIN(concurrency-mt-unsafe)

BOOST_AUTO_TEST_SUITE(file_list_caching_test_suite)
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

    // Create a file list
    const std::vector<sFile> fileListData = { // NOLINT(cert-err58-cpp)
            {"/", 0, 0, true},
            {"/test", randomInt(0, static_cast<uint64_t>(-1)), 0, false},
            {"/testdir", 0, 0, true},
            {"/testdir/file", randomInt(0, static_cast<uint64_t>(-1)), 0, false},
            {"/testdir/file2", randomInt(0, static_cast<uint64_t>(-1)), 0, false},
            {"/testdir/file3", randomInt(0, static_cast<uint64_t>(-1)), 0, false},
            {"/testdir/testdir1", 0, 0, true},
            {"/testdir/testdir1/file", randomInt(0, static_cast<uint64_t>(-1)), 0, false},
            {"/test2", randomInt(0, static_cast<uint64_t>(-1)), 0, false},
    };

    BOOST_AUTO_TEST_CASE(test_file_list) { // NOLINT(readability-function-cognitive-complexity)
        // Delete all database info just in case
        auto database = MySqlConnector();
        schema::JobserverFiledownload fileDownloadTable;
        schema::JobserverFilelistcache fileListCacheTable;
        schema::JobserverJob jobTable;
        schema::JobserverJobhistory jobHistoryTable;
        schema::JobserverClusteruuid clusterUuidTable;
        database->run(remove_from(fileListCacheTable).unconditionally());
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
        bool bRaiseError = true;
        bool bReady = false;
        std::vector<std::string> lastDirPath;
        std::vector<bool> lastbRecursive;
        TestWsClient websocketClient("localhost:8001/job/ws/?token=" + getLastToken());
        websocketClient.on_message = [&lastDirPath, &lastbRecursive, &bRaiseError, &bReady](const std::shared_ptr<TestWsClient::Connection>& connection, const std::shared_ptr<TestWsClient::InMessage>& in_message) {
            auto data = in_message->string();
            auto msg = Message(std::vector<uint8_t>(data.begin(), data.end()));

            // Ignore the ready message
            if (msg.getId() == SERVER_READY) {
                bReady = true;
                return;
            }

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

                auto outMessage = std::make_shared<TestWsClient::OutMessage>(msg.getdata()->get()->size());
                std::ostream_iterator<uint8_t> iter(*outMessage);
                std::copy(msg.getdata()->get()->begin(), msg.getdata()->get()->end(), iter);
                // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
                connection->send(outMessage, nullptr, 130);
                return;
            }

            lastDirPath.push_back(msg.pop_string());
            lastbRecursive.push_back(msg.pop_bool());

            auto filteredFiles = filterFiles(fileListData, lastDirPath.back(), lastbRecursive.back());

            msg = Message(FILE_LIST, Message::Priority::Highest, "");
            msg.push_string(uuid);
            msg.push_uint(filteredFiles.size());

            for (const auto &file : filteredFiles) {
                msg.push_string(file.fileName);
                msg.push_bool(file.isDirectory);
                msg.push_ulong(file.fileSize);
            }

            auto outStream = std::make_shared<TestWsClient::OutMessage>(msg.getdata()->get()->size());
            std::ostream_iterator<uint8_t> iter(*outStream);
            std::copy(msg.getdata()->get()->begin(), msg.getdata()->get()->end(), iter);
            // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
            connection->send(outStream, nullptr, 130);
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

        // Create a job to request file lists for
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

        // Set up the auth token
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
                {"recursive", false},
                {"path",  "/testdir"}
        };

        // This call should cause an error to be raised and reported
        TestHttpClient httpClient("localhost:8000");
        auto response = httpClient.request("PATCH", "/job/apiv1/file/", params.dump(), {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(std::stoi(response->status_code), (int) SimpleWeb::StatusCode::client_error_bad_request);
        BOOST_CHECK_EQUAL(response->content.string(), "test error");
        BOOST_CHECK_EQUAL(fileListMap->size(), 0);

        // Finish error testing
        bRaiseError = false;

        // This file list will be complete, but won't create any file cache objects because the job has no
        // _job_completion_ job history objects
        response = httpClient.request("PATCH", "/job/apiv1/file/", params.dump(), {{"Authorization", jwtToken.signature()}});

        BOOST_CHECK_EQUAL(fileListMap->size(), 0);

        BOOST_CHECK_EQUAL(lastDirPath.size(), 1);
        BOOST_CHECK_EQUAL(lastDirPath.back(), std::string(params["path"]));
        BOOST_CHECK_EQUAL(lastbRecursive.back(), (bool) params["recursive"]);

        // NOLINTBEGIN(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
        std::vector<sFile> expected = {
                fileListData[2],
                fileListData[3],
                fileListData[4],
                fileListData[5],
                fileListData[6]
        };
        // NOLINTEND(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)

        nlohmann::json result;
        response->content >> result;
        BOOST_CHECK(result.find("files") != result.end());
        auto jsonData = result["files"];

        // Check that the file list returned was correct
        for (auto index = 0; index < expected.size(); index++) {
            auto file = expected[index];

            BOOST_CHECK_EQUAL(jsonData[index]["path"], file.fileName);
            BOOST_CHECK_EQUAL(jsonData[index]["isDir"], file.isDirectory);
            BOOST_CHECK_EQUAL(jsonData[index]["fileSize"], file.fileSize);
            BOOST_CHECK_EQUAL(jsonData[index]["permissions"], file.permissions);
        }

        // Next we want to create a _job_completion_ file history object and request files from a sub directory
        database->run(
                insert_into(jobHistoryTable)
                        .set(
                                jobHistoryTable.jobId = jobId,
                                jobHistoryTable.timestamp = std::chrono::system_clock::now(),
                                jobHistoryTable.what = "_job_completion_",
                                jobHistoryTable.state = static_cast<uint32_t>(JobStatus::COMPLETED),
                                jobHistoryTable.details = "Job is complete"
                        )
        );

        // Reset the websocket trackers
        lastDirPath.clear();
        lastbRecursive.clear();

        // This file list will be complete, and should also create the file list cache since now a _job_completion_ job
        // history record exists.
        response = httpClient.request("PATCH", "/job/apiv1/file/", params.dump(), {{"Authorization", jwtToken.signature()}});

        // There should have been two websocket calls - one to get the file list of the initial request, and a second
        // to get the full recursive file list of the entire job
        BOOST_CHECK_EQUAL(lastDirPath.size(), 2);
        BOOST_CHECK_EQUAL(lastDirPath[0], std::string(params["path"]));
        BOOST_CHECK_EQUAL(lastDirPath[1], "");
        BOOST_CHECK_EQUAL(lastbRecursive[0], (bool) params["recursive"]);
        BOOST_CHECK_EQUAL(lastbRecursive[1], true);

        response->content >> result;
        BOOST_CHECK(result.find("files") != result.end());
        jsonData = result["files"];

        // Check that the file list returned was correct
        for (auto index = 0; index < expected.size(); index++) {
            auto file = expected[index];

            BOOST_CHECK_EQUAL(jsonData[index]["path"], file.fileName);
            BOOST_CHECK_EQUAL(jsonData[index]["isDir"], file.isDirectory);
            BOOST_CHECK_EQUAL(jsonData[index]["fileSize"], file.fileSize);
            BOOST_CHECK_EQUAL(jsonData[index]["permissions"], file.permissions);
        }

        // Check that the correct file list cache entries were added to the database
        auto fileListCacheResult = database->run(
                select(all_of(fileListCacheTable))
                        .from(fileListCacheTable)
                        .where(
                                fileListCacheTable.jobId == static_cast<uint32_t>(jobId)
                        )
        );

        BOOST_CHECK_EQUAL(fileListCacheResult.empty(), false);

        std::vector<sFile> files;
        for (const auto &file : fileListCacheResult) {
            files.push_back(
                {
                    file.path,
                    static_cast<uint64_t>(file.fileSize),
                    static_cast<uint32_t>(file.permissions),
                    static_cast<bool>(file.isDir)
                }
                );
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
        response = httpClient.request("PATCH", "/job/apiv1/file/", params.dump(), {{"Authorization", jwtToken.signature()}});

        // There should have been no websocket calls
        BOOST_CHECK_EQUAL(lastDirPath.size(), 0);

        response->content >> result;
        BOOST_CHECK(result.find("files") != result.end());
        jsonData = result["files"];

        // Check that the file list returned was correct
        for (auto index = 0; index < expected.size(); index++) {
            auto file = expected[index];

            BOOST_CHECK_EQUAL(jsonData[index]["path"], file.fileName);
            BOOST_CHECK_EQUAL(jsonData[index]["isDir"], file.isDirectory);
            BOOST_CHECK_EQUAL(jsonData[index]["fileSize"], file.fileSize);
            BOOST_CHECK_EQUAL(jsonData[index]["permissions"], file.permissions);
        }

        params = {
                {"jobId", jobId},
                {"recursive", true},
                {"path",  "/"}
        };

        // This file list will be complete, and should not call the websocket since all files should be cached
        response = httpClient.request("PATCH", "/job/apiv1/file/", params.dump(), {{"Authorization", jwtToken.signature()}});

        // There should have been no websocket calls
        BOOST_CHECK_EQUAL(lastDirPath.size(), 0);

        response->content >> result;
        BOOST_CHECK(result.find("files") != result.end());
        jsonData = result["files"];

        // Check that the file list returned was correct
        for (auto index = 0; index < fileListData.size(); index++) {
            auto file = fileListData[index];

            BOOST_CHECK_EQUAL(jsonData[index]["path"], file.fileName);
            BOOST_CHECK_EQUAL(jsonData[index]["isDir"], file.isDirectory);
            BOOST_CHECK_EQUAL(jsonData[index]["fileSize"], file.fileSize);
            BOOST_CHECK_EQUAL(jsonData[index]["permissions"], file.permissions);
        }

        params = {
                {"jobId", jobId},
                {"recursive", true},
                {"path",  "/testdir"}
        };

        // This file list will be complete, and should not call the websocket since all files should be cached
        response = httpClient.request("PATCH", "/job/apiv1/file/", params.dump(), {{"Authorization", jwtToken.signature()}});

        // There should have been no websocket calls
        BOOST_CHECK_EQUAL(lastDirPath.size(), 0);

        response->content >> result;
        BOOST_CHECK(result.find("files") != result.end());
        jsonData = result["files"];

        // NOLINTBEGIN(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
        expected = {
                fileListData[2],
                fileListData[3],
                fileListData[4],
                fileListData[5],
                fileListData[6],
                fileListData[7]
        };
        // NOLINTEND(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)

        // Check that the file list returned was correct
        for (auto index = 0; index < expected.size(); index++) {
            auto file = expected[index];

            BOOST_CHECK_EQUAL(jsonData[index]["path"], file.fileName);
            BOOST_CHECK_EQUAL(jsonData[index]["isDir"], file.isDirectory);
            BOOST_CHECK_EQUAL(jsonData[index]["fileSize"], file.fileSize);
            BOOST_CHECK_EQUAL(jsonData[index]["permissions"], file.permissions);
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
        database->run(remove_from(fileListCacheTable).unconditionally());
        database->run(remove_from(jobHistoryTable).unconditionally());
        database->run(remove_from(jobTable).unconditionally());
    }

    BOOST_AUTO_TEST_CASE(test_job_finished_update) { // NOLINT(readability-function-cognitive-complexity)
        // Delete all database info just in case
        auto database = MySqlConnector();
        schema::JobserverFiledownload fileDownloadTable;
        schema::JobserverFilelistcache fileListCacheTable;
        schema::JobserverJob jobTable;
        schema::JobserverJobhistory jobHistoryTable;
        schema::JobserverClusteruuid clusterUuidTable;
        database->run(remove_from(fileListCacheTable).unconditionally());
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
        bool bReady = false;
        std::vector<std::string> lastDirPath;
        std::vector<bool> lastbRecursive;
        TestWsClient websocketClient("localhost:8001/job/ws/?token=" + getLastToken());
        websocketClient.on_message = [&lastDirPath, &lastbRecursive, &bReady](const std::shared_ptr<TestWsClient::Connection>& connection, const std::shared_ptr<TestWsClient::InMessage>& in_message) {
            auto data = in_message->string();
            auto msg = Message(std::vector<uint8_t>(data.begin(), data.end()));

            // Ignore the ready message
            if (msg.getId() == SERVER_READY) {
                bReady = true;
                return;
            }

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

            for (auto &file : filteredFiles) {
                msg.push_string(file.fileName);
                msg.push_bool(file.isDirectory);
                msg.push_ulong(file.fileSize);
            }

            auto outStream = std::make_shared<TestWsClient::OutMessage>(msg.getdata()->get()->size());
            std::ostream_iterator<uint8_t> iter(*outStream);
            std::copy(msg.getdata()->get()->begin(), msg.getdata()->get()->end(), iter);
            // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
            connection->send(outStream, nullptr, 130);
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

        // Create a job to request file lists for
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

        // Create a file list
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
                {"recursive", false},
                {"path",  "/testdir"}
        };

        // This file list will be complete, but won't create any file cache objects because the job has no
        // _job_completion_ job history objects
        TestHttpClient httpClient("localhost:8000");
        auto response = httpClient.request("PATCH", "/job/apiv1/file/", params.dump(), {{"Authorization", jwtToken.signature()}});

        BOOST_CHECK_EQUAL(lastDirPath.size(), 1);
        BOOST_CHECK_EQUAL(lastDirPath.back(), std::string(params["path"]));
        BOOST_CHECK_EQUAL(lastbRecursive.back(), (bool) params["recursive"]);

        // NOLINTBEGIN(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
        std::vector<sFile> expected = {
                fileListData[2],
                fileListData[3],
                fileListData[4],
                fileListData[5],
                fileListData[6]
        };
        // NOLINTEND(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)

        nlohmann::json result;
        response->content >> result;
        BOOST_CHECK(result.find("files") != result.end());
        auto jsonData = result["files"];

        // Check that the file list returned was correct
        for (auto index = 0; index < expected.size(); index++) {
            auto file = expected[index];

            BOOST_CHECK_EQUAL(jsonData[index]["path"], file.fileName);
            BOOST_CHECK_EQUAL(jsonData[index]["isDir"], file.isDirectory);
            BOOST_CHECK_EQUAL(jsonData[index]["fileSize"], file.fileSize);
            BOOST_CHECK_EQUAL(jsonData[index]["permissions"], file.permissions);
        }

        // Reset the websocket trackers
        lastDirPath.clear();
        lastbRecursive.clear();

        // Now if we update the job status with a status other than _job_completion_,
        // then all files should still be read from the websocket
        Message msg(UPDATE_JOB);
        msg.push_uint(jobId);                   // jobId
        msg.push_string("running");             // what
        msg.push_uint(static_cast<uint32_t>(JobStatus::COMPLETED)); // status
        msg.push_string("it's fine");           // details
        mgr->getvClusters()->at(0)->handleMessage(msg);

        response = httpClient.request("PATCH", "/job/apiv1/file/", params.dump(), {{"Authorization", jwtToken.signature()}});

        BOOST_CHECK_EQUAL(lastDirPath.size(), 1);
        BOOST_CHECK_EQUAL(lastDirPath.back(), std::string(params["path"]));
        BOOST_CHECK_EQUAL(lastbRecursive.back(), (bool) params["recursive"]);

        response->content >> result;
        BOOST_CHECK(result.find("files") != result.end());
        jsonData = result["files"];

        // Check that the file list returned was correct
        for (auto index = 0; index < expected.size(); index++) {
            auto file = expected[index];

            BOOST_CHECK_EQUAL(jsonData[index]["path"], file.fileName);
            BOOST_CHECK_EQUAL(jsonData[index]["isDir"], file.isDirectory);
            BOOST_CHECK_EQUAL(jsonData[index]["fileSize"], file.fileSize);
            BOOST_CHECK_EQUAL(jsonData[index]["permissions"], file.permissions);
        }

        // Reset the websocket trackers
        lastDirPath.clear();
        lastbRecursive.clear();

        // Now if we update the job status with a status for _job_completion_, then all files should be read from
        // the file list cache
        msg = Message(UPDATE_JOB);
        msg.push_uint(jobId);                   // jobId
        msg.push_string("_job_completion_");    // what
        msg.push_uint(static_cast<uint32_t>(JobStatus::ERROR)); // status
        msg.push_string("it's fine");           // details
        mgr->getvClusters()->at(0)->handleMessage(msg);

        BOOST_CHECK_EQUAL(lastDirPath.size(), 1);
        BOOST_CHECK_EQUAL(lastDirPath.back(), "");
        BOOST_CHECK_EQUAL(lastbRecursive.back(), true);

        // Reset the websocket trackers
        lastDirPath.clear();
        lastbRecursive.clear();

        // This file list will be complete, and should read files from only the file cache
        response = httpClient.request("PATCH", "/job/apiv1/file/", params.dump(), {{"Authorization", jwtToken.signature()}});

        // There should have been two websocket calls - one to get the file list of the initial request, and a second
        // to get the full recursive file list of the entire job
        BOOST_CHECK_EQUAL(lastDirPath.size(), 0);

        response->content >> result;
        BOOST_CHECK(result.find("files") != result.end());
        jsonData = result["files"];

        // Check that the file list returned was correct
        for (auto index = 0; index < expected.size(); index++) {
            auto file = expected[index];

            BOOST_CHECK_EQUAL(jsonData[index]["path"], file.fileName);
            BOOST_CHECK_EQUAL(jsonData[index]["isDir"], file.isDirectory);
            BOOST_CHECK_EQUAL(jsonData[index]["fileSize"], file.fileSize);
            BOOST_CHECK_EQUAL(jsonData[index]["permissions"], file.permissions);
        }

        // Check that the correct file list cache entries were added to the database
        auto fileListCacheResult = database->run(
                select(all_of(fileListCacheTable))
                        .from(fileListCacheTable)
                        .where(
                                fileListCacheTable.jobId == static_cast<uint32_t>(jobId)
                        )
        );

        BOOST_CHECK_EQUAL(fileListCacheResult.empty(), false);

        std::vector<sFile> files;
        for (const auto &file : fileListCacheResult) {
            files.push_back(
                {
                    file.path,
                    static_cast<uint64_t>(file.fileSize),
                    static_cast<uint32_t>(file.permissions),
                    static_cast<bool>(file.isDir)
                }
            );
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
        response = httpClient.request("PATCH", "/job/apiv1/file/", params.dump(), {{"Authorization", jwtToken.signature()}});

        // There should have been no websocket calls
        BOOST_CHECK_EQUAL(lastDirPath.size(), 0);

        response->content >> result;
        BOOST_CHECK(result.find("files") != result.end());
        jsonData = result["files"];

        // Check that the file list returned was correct
        for (auto index = 0; index < expected.size(); index++) {
            auto file = expected[index];

            BOOST_CHECK_EQUAL(jsonData[index]["path"], file.fileName);
            BOOST_CHECK_EQUAL(jsonData[index]["isDir"], file.isDirectory);
            BOOST_CHECK_EQUAL(jsonData[index]["fileSize"], file.fileSize);
            BOOST_CHECK_EQUAL(jsonData[index]["permissions"], file.permissions);
        }

        params = {
                {"jobId", jobId},
                {"recursive", true},
                {"path",  "/"}
        };

        // This file list will be complete, and should not call the websocket since all files should be cached
        response = httpClient.request("PATCH", "/job/apiv1/file/", params.dump(), {{"Authorization", jwtToken.signature()}});

        // There should have been no websocket calls
        BOOST_CHECK_EQUAL(lastDirPath.size(), 0);

        response->content >> result;
        BOOST_CHECK(result.find("files") != result.end());
        jsonData = result["files"];

        // Check that the file list returned was correct
        for (auto index = 0; index < fileListData.size(); index++) {
            auto file = fileListData[index];

            BOOST_CHECK_EQUAL(jsonData[index]["path"], file.fileName);
            BOOST_CHECK_EQUAL(jsonData[index]["isDir"], file.isDirectory);
            BOOST_CHECK_EQUAL(jsonData[index]["fileSize"], file.fileSize);
            BOOST_CHECK_EQUAL(jsonData[index]["permissions"], file.permissions);
        }

        params = {
                {"jobId", jobId},
                {"recursive", true},
                {"path",  "/testdir"}
        };

        // This file list will be complete, and should not call the websocket since all files should be cached
        response = httpClient.request("PATCH", "/job/apiv1/file/", params.dump(), {{"Authorization", jwtToken.signature()}});

        // There should have been no websocket calls
        BOOST_CHECK_EQUAL(lastDirPath.size(), 0);

        response->content >> result;
        BOOST_CHECK(result.find("files") != result.end());
        jsonData = result["files"];

        // NOLINTBEGIN(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
        expected = {
                fileListData[2],
                fileListData[3],
                fileListData[4],
                fileListData[5],
                fileListData[6],
                fileListData[7]
        };
        // NOLINTEND(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)

        // Check that the file list returned was correct
        for (auto index = 0; index < expected.size(); index++) {
            auto file = expected[index];

            BOOST_CHECK_EQUAL(jsonData[index]["path"], file.fileName);
            BOOST_CHECK_EQUAL(jsonData[index]["isDir"], file.isDirectory);
            BOOST_CHECK_EQUAL(jsonData[index]["fileSize"], file.fileSize);
            BOOST_CHECK_EQUAL(jsonData[index]["permissions"], file.permissions);
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
        database->run(remove_from(fileListCacheTable).unconditionally());
        database->run(remove_from(jobHistoryTable).unconditionally());
        database->run(remove_from(jobTable).unconditionally());
    }
BOOST_AUTO_TEST_SUITE_END()

// NOLINTEND(concurrency-mt-unsafe)