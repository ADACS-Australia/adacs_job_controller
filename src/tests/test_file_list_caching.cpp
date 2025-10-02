//
// Created by lewis on 10/6/21.
//


import settings;
import job_status;
#include <boost/test/unit_test.hpp>
#include <jwt/jwt.hpp>
#include <sqlpp11/sqlpp11.h>

#include "../../Lib/shims/sqlpp_shim.h"
#include "../../tests/fixtures/DatabaseFixture.h"
#include "../../tests/fixtures/HttpClientFixture.h"
#include "../../tests/fixtures/WebSocketClientFixture.h"
#include "../Lib/FileTypes.h"

import HandleFileList;
import Message;
import IApplication;
import HttpServer;
import ClusterManager;

struct FileListTestDataFixture : public DatabaseFixture, public WebSocketClientFixture, public HttpClientFixture
{
    uint64_t jobId;
    bool bRaiseError = true;
    bool bReady      = false;
    std::vector<std::string> lastDirPath;
    std::vector<bool> lastbRecursive;

    // Create a file list
    const std::vector<sFile> fileListData = {

        {.fileName = "/", .fileSize = 0, .permissions = 0, .isDirectory = true},
        {.fileName    = "/test",
         .fileSize    = randomInt(0, static_cast<uint64_t>(-1)),
         .permissions = 0,
         .isDirectory = false},
        {.fileName = "/testdir", .fileSize = 0, .permissions = 0, .isDirectory = true},
        {.fileName    = "/testdir/file",
         .fileSize    = randomInt(0, static_cast<uint64_t>(-1)),
         .permissions = 0,
         .isDirectory = false},
        {.fileName    = "/testdir/file2",
         .fileSize    = randomInt(0, static_cast<uint64_t>(-1)),
         .permissions = 0,
         .isDirectory = false},
        {.fileName    = "/testdir/file3",
         .fileSize    = randomInt(0, static_cast<uint64_t>(-1)),
         .permissions = 0,
         .isDirectory = false},
        {.fileName = "/testdir/testdir1", .fileSize = 0, .permissions = 0, .isDirectory = true},
        {.fileName    = "/testdir/testdir1/file",
         .fileSize    = randomInt(0, static_cast<uint64_t>(-1)),
         .permissions = 0,
         .isDirectory = false},
        {.fileName    = "/test2",
         .fileSize    = randomInt(0, static_cast<uint64_t>(-1)),
         .permissions = 0,
         .isDirectory = false},
    };

    FileListTestDataFixture()
    {
        // Fabricate data
        // Create a job to request file lists for
        jobId = database->run(insert_into(jobTable).set(
            jobTable.user       = 1,
            jobTable.parameters = "params1",
            jobTable.cluster = std::static_pointer_cast<HttpServer>(httpServer)->getvJwtSecrets()->at(2).clusters()[0],
            jobTable.bundle  = "my_test_bundle",
            jobTable.application = std::static_pointer_cast<HttpServer>(httpServer)->getvJwtSecrets()->at(0).name()));

        websocketClient->on_message = [&](const auto& connection, const auto& in_message) {
            onWebsocketMessage(connection, in_message);
        };

        startWebSocketClient();

        // Wait for the client to connect
        while (!bReady)
        {
            std::this_thread::sleep_for(std::chrono::milliseconds(10));
        }
    }

    void onWebsocketMessage(const auto& connection, const auto& in_message)
    {
        auto data = in_message->string();
        Message msg(std::vector<uint8_t>(data.begin(), data.end()));

        // Ignore the ready message
        if (msg.getId() == SERVER_READY)
        {
            bReady = true;
            return;
        }

        // Check that this message can only be a FILE_LIST request
        if (msg.getId() != FILE_LIST)
        {
            BOOST_ASSERT_MSG(false, "Got unexpected websocket message");
            return;
        }

        auto wsJobId    = msg.pop_uint();
        auto uuid       = msg.pop_string();
        auto bundleHash = msg.pop_string();
        auto filePath   = msg.pop_string();
        auto bRecursive = msg.pop_bool();

        // The job id passed on the websocket should always be 0
        BOOST_CHECK_EQUAL(wsJobId, jobId);

        // The bundle hash should always equal my_test_bundle
        BOOST_CHECK_EQUAL(bundleHash, "my_test_bundle");

        // Check if we should raise an error
        if (bRaiseError)
        {
            msg = Message(FILE_LIST_ERROR, Message::Priority::Highest, "");
            msg.push_string(uuid);
            msg.push_string("test error");

            auto outMessage = std::make_shared<TestWsClient::OutMessage>(msg.getdata()->get()->size());
            const std::ostream_iterator<uint8_t> iter(*outMessage);
            std::copy(msg.getdata()->get()->begin(), msg.getdata()->get()->end(), iter);

            connection->send(outMessage, nullptr, 130);
            return;
        }

        lastDirPath.push_back(filePath);
        lastbRecursive.push_back(bRecursive);

        auto filteredFiles = filterFiles(fileListData, lastDirPath.back(), lastbRecursive.back());

        msg = Message(FILE_LIST, Message::Priority::Highest, "");
        msg.push_string(uuid);
        msg.push_uint(filteredFiles.size());

        for (const auto& file : filteredFiles)
        {
            msg.push_string(file.fileName);
            msg.push_bool(file.isDirectory);
            msg.push_ulong(file.fileSize);
        }

        auto outStream = std::make_shared<TestWsClient::OutMessage>(msg.getdata()->get()->size());
        const std::ostream_iterator<uint8_t> iter(*outStream);
        std::copy(msg.getdata()->get()->begin(), msg.getdata()->get()->end(), iter);

        connection->send(outStream, nullptr, 130);
    }
};

BOOST_FIXTURE_TEST_SUITE(file_list_caching_test_suite, FileListTestDataFixture)

BOOST_AUTO_TEST_CASE(test_file_list)
{
    setJwtSecret(std::static_pointer_cast<HttpServer>(httpServer)->getvJwtSecrets()->at(0).secret());

    // Create jsonParams
    jsonParams = {
        {    "jobId",      jobId},
        {"recursive",      false},
        {     "path", "/testdir"}
    };

    // This call should cause an error to be raised and reported
    auto response = httpClient.request("PATCH",
                                       "/job/apiv1/file/",
                                       jsonParams.dump(),
                                       {
                                           {"Authorization", jwtToken.signature()}
    });
    BOOST_CHECK_EQUAL(std::stoi(response->status_code), (int)SimpleWeb::StatusCode::client_error_bad_request);
    BOOST_CHECK_EQUAL(response->content.string(), "test error");
    auto fileListMap1 = application->getFileListMap();
    BOOST_CHECK_EQUAL(fileListMap1->size(), 0);

    // Finish error testing
    bRaiseError = false;

    // This file list will be complete, but won't create any file cache objects because the job has no
    // _job_completion_ job history objects
    response = httpClient.request("PATCH",
                                  "/job/apiv1/file/",
                                  jsonParams.dump(),
                                  {
                                      {"Authorization", jwtToken.signature()}
    });

    auto fileListMap2 = application->getFileListMap();
    BOOST_CHECK_EQUAL(fileListMap2->size(), 0);

    BOOST_CHECK_EQUAL(lastDirPath.size(), 1);
    BOOST_CHECK_EQUAL(lastDirPath.back(), std::string(jsonParams["path"]));
    BOOST_CHECK_EQUAL(lastbRecursive.back(), (bool)jsonParams["recursive"]);


    std::vector<sFile> expected = {fileListData[2], fileListData[3], fileListData[4], fileListData[5], fileListData[6]};


    response->content >> jsonResult;
    BOOST_CHECK(jsonResult.contains("files"));
    auto jsonData = jsonResult["files"];

    // Check that the file list returned was correct
    for (auto index = 0; index < expected.size(); index++)
    {
        const auto& file = expected[index];

        BOOST_CHECK_EQUAL(jsonData[index]["path"], file.fileName);
        BOOST_CHECK_EQUAL(jsonData[index]["isDir"], file.isDirectory);
        BOOST_CHECK_EQUAL(jsonData[index]["fileSize"], file.fileSize);
        BOOST_CHECK_EQUAL(jsonData[index]["permissions"], file.permissions);
    }

    // Next we want to create a _job_completion_ file history object and request files from a sub directory
    database->run(insert_into(jobHistoryTable)
                      .set(jobHistoryTable.jobId     = jobId,
                           jobHistoryTable.timestamp = std::chrono::system_clock::now(),
                           jobHistoryTable.what      = "_job_completion_",
                           jobHistoryTable.state     = static_cast<uint32_t>(JobStatus::COMPLETED),
                           jobHistoryTable.details   = "Job is complete"));

    // Reset the websocket trackers
    lastDirPath.clear();
    lastbRecursive.clear();

    // This file list will be complete, and should also create the file list cache since now a _job_completion_ job
    // history record exists.
    response = httpClient.request("PATCH",
                                  "/job/apiv1/file/",
                                  jsonParams.dump(),
                                  {
                                      {"Authorization", jwtToken.signature()}
    });

    // Wait until both websocket calls are made
    int counter = 0;
    // 500 * 10ms = 5 seconds.

    while ((lastDirPath.size() < 2 || lastbRecursive.size() < 2) && counter < 500)
    {
        std::this_thread::sleep_for(std::chrono::milliseconds(10));
        counter++;
    }

    BOOST_ASSERT_MSG(counter != 500, "Websocket took too long to respond");

    // There should have been two websocket calls - one to get the file list of the initial request, and a second
    // to get the full recursive file list of the entire job
    BOOST_CHECK_EQUAL(lastDirPath.size(), 2);
    BOOST_CHECK_EQUAL(lastDirPath[0], std::string(jsonParams["path"]));
    BOOST_CHECK_EQUAL(lastDirPath[1], "");
    BOOST_CHECK_EQUAL(lastbRecursive[0], (bool)jsonParams["recursive"]);
    BOOST_CHECK_EQUAL(lastbRecursive[1], true);

    response->content >> jsonResult;
    BOOST_CHECK(jsonResult.contains("files"));
    jsonData = jsonResult["files"];

    // Check that the file list returned was correct
    for (auto index = 0; index < expected.size(); index++)
    {
        const auto& file = expected[index];

        BOOST_CHECK_EQUAL(jsonData[index]["path"], file.fileName);
        BOOST_CHECK_EQUAL(jsonData[index]["isDir"], file.isDirectory);
        BOOST_CHECK_EQUAL(jsonData[index]["fileSize"], file.fileSize);
        BOOST_CHECK_EQUAL(jsonData[index]["permissions"], file.permissions);
    }

    // The file list caching can take a moment to complete in the background
    // Check that the correct file list cache entries were added to the database

    for (counter = 0; counter < 500; counter++)
    {
        auto fileListCacheResult = database->run(
            select(all_of(jobFilelistcache)).from(jobFilelistcache).where(jobFilelistcache.jobId == jobId));

        // Has the file list cache finished being populated by the background system thread?
        if (!fileListCacheResult.empty())
        {
            break;
        }


        std::this_thread::sleep_for(std::chrono::milliseconds(10));
    }

    // Fail if counter = 500
    BOOST_ASSERT_MSG(counter != 500, "Background file list caching thread failed to run correctly");

    auto fileListCacheResult =
        database->run(select(all_of(jobFilelistcache)).from(jobFilelistcache).where(jobFilelistcache.jobId == jobId));

    std::vector<sFile> files;
    for (const auto& file : fileListCacheResult)
    {
        files.push_back({.fileName    = file.path,
                         .fileSize    = static_cast<uint64_t>(file.fileSize),
                         .permissions = static_cast<uint32_t>(file.permissions),
                         .isDirectory = static_cast<bool>(file.isDir)});
    }

    for (auto i = 0; i < files.size(); i++)
    {
        BOOST_CHECK_EQUAL(files[i].fileName, fileListData[i].fileName);
        BOOST_CHECK_EQUAL(files[i].isDirectory, fileListData[i].isDirectory);
        BOOST_CHECK_EQUAL(files[i].fileSize, fileListData[i].fileSize);
        BOOST_CHECK_EQUAL(files[i].permissions, fileListData[i].permissions);
    }

    // Reset the websocket trackers
    lastDirPath.clear();
    lastbRecursive.clear();

    // This file list will be complete, and should not call the websocket since all files should be cached
    response = httpClient.request("PATCH",
                                  "/job/apiv1/file/",
                                  jsonParams.dump(),
                                  {
                                      {"Authorization", jwtToken.signature()}
    });

    // There should have been no websocket calls
    BOOST_CHECK_EQUAL(lastDirPath.size(), 0);

    response->content >> jsonResult;
    BOOST_CHECK(jsonResult.contains("files"));
    jsonData = jsonResult["files"];

    // Check that the file list returned was correct
    for (auto index = 0; index < expected.size(); index++)
    {
        const auto& file = expected[index];

        BOOST_CHECK_EQUAL(jsonData[index]["path"], file.fileName);
        BOOST_CHECK_EQUAL(jsonData[index]["isDir"], file.isDirectory);
        BOOST_CHECK_EQUAL(jsonData[index]["fileSize"], file.fileSize);
        BOOST_CHECK_EQUAL(jsonData[index]["permissions"], file.permissions);
    }

    jsonParams = {
        {    "jobId", jobId},
        {"recursive",  true},
        {     "path",   "/"}
    };

    // This file list will be complete, and should not call the websocket since all files should be cached
    response = httpClient.request("PATCH",
                                  "/job/apiv1/file/",
                                  jsonParams.dump(),
                                  {
                                      {"Authorization", jwtToken.signature()}
    });

    // There should have been no websocket calls
    BOOST_CHECK_EQUAL(lastDirPath.size(), 0);

    response->content >> jsonResult;
    BOOST_CHECK(jsonResult.contains("files"));
    jsonData = jsonResult["files"];

    // Check that the file list returned was correct
    for (auto index = 0; index < fileListData.size(); index++)
    {
        auto file = fileListData[index];

        BOOST_CHECK_EQUAL(jsonData[index]["path"], file.fileName);
        BOOST_CHECK_EQUAL(jsonData[index]["isDir"], file.isDirectory);
        BOOST_CHECK_EQUAL(jsonData[index]["fileSize"], file.fileSize);
        BOOST_CHECK_EQUAL(jsonData[index]["permissions"], file.permissions);
    }

    jsonParams = {
        {    "jobId",      jobId},
        {"recursive",       true},
        {     "path", "/testdir"}
    };

    // This file list will be complete, and should not call the websocket since all files should be cached
    response = httpClient.request("PATCH",
                                  "/job/apiv1/file/",
                                  jsonParams.dump(),
                                  {
                                      {"Authorization", jwtToken.signature()}
    });

    // There should have been no websocket calls
    BOOST_CHECK_EQUAL(lastDirPath.size(), 0);

    response->content >> jsonResult;
    BOOST_CHECK(jsonResult.contains("files"));
    jsonData = jsonResult["files"];


    expected = {fileListData[2], fileListData[3], fileListData[4], fileListData[5], fileListData[6], fileListData[7]};


    // Check that the file list returned was correct
    for (auto index = 0; index < expected.size(); index++)
    {
        const auto& file = expected[index];

        BOOST_CHECK_EQUAL(jsonData[index]["path"], file.fileName);
        BOOST_CHECK_EQUAL(jsonData[index]["isDir"], file.isDirectory);
        BOOST_CHECK_EQUAL(jsonData[index]["fileSize"], file.fileSize);
        BOOST_CHECK_EQUAL(jsonData[index]["permissions"], file.permissions);
    }
}

BOOST_AUTO_TEST_CASE(test_job_finished_update)
{
    setJwtSecret(std::static_pointer_cast<HttpServer>(httpServer)->getvJwtSecrets()->at(0).secret());

    // Create jsonParams
    jsonParams = {
        {    "jobId",      jobId},
        {"recursive",      false},
        {     "path", "/testdir"}
    };

    // Don't raise any error in the websocket handler
    bRaiseError = false;

    // This file list will be complete, but won't create any file cache objects because the job has no
    // _job_completion_ job history objects
    auto response = httpClient.request("PATCH",
                                       "/job/apiv1/file/",
                                       jsonParams.dump(),
                                       {
                                           {"Authorization", jwtToken.signature()}
    });

    BOOST_CHECK_EQUAL(lastDirPath.size(), 1);
    BOOST_CHECK_EQUAL(lastDirPath.back(), std::string(jsonParams["path"]));
    BOOST_CHECK_EQUAL(lastbRecursive.back(), (bool)jsonParams["recursive"]);


    std::vector<sFile> expected = {fileListData[2], fileListData[3], fileListData[4], fileListData[5], fileListData[6]};


    response->content >> jsonResult;
    BOOST_CHECK(jsonResult.contains("files"));
    auto jsonData = jsonResult["files"];

    // Check that the file list returned was correct
    for (auto index = 0; index < expected.size(); index++)
    {
        const auto& file = expected[index];

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
    msg.push_uint(jobId);                                        // jobId
    msg.push_string("running");                                  // what
    msg.push_uint(static_cast<uint32_t>(JobStatus::COMPLETED));  // status
    msg.push_string("it's fine");                                // details
    std::static_pointer_cast<ClusterManager>(clusterManager)->getvClusters()->at(0)->handleMessage(msg);

    response = httpClient.request("PATCH",
                                  "/job/apiv1/file/",
                                  jsonParams.dump(),
                                  {
                                      {"Authorization", jwtToken.signature()}
    });

    BOOST_CHECK_EQUAL(lastDirPath.size(), 1);
    BOOST_CHECK_EQUAL(lastDirPath.back(), std::string(jsonParams["path"]));
    BOOST_CHECK_EQUAL(lastbRecursive.back(), (bool)jsonParams["recursive"]);

    response->content >> jsonResult;
    BOOST_CHECK(jsonResult.contains("files"));
    jsonData = jsonResult["files"];

    // Check that the file list returned was correct
    for (auto index = 0; index < expected.size(); index++)
    {
        const auto& file = expected[index];

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
    msg.push_uint(jobId);                                    // jobId
    msg.push_string("_job_completion_");                     // what
    msg.push_uint(static_cast<uint32_t>(JobStatus::ERROR));  // status
    msg.push_string("it's fine");                            // details
    std::static_pointer_cast<ClusterManager>(clusterManager)->getvClusters()->at(0)->handleMessage(msg);

    // Wait until the file list websocket call is made (Should be triggered because of the job completion status
    int counter = 0;
    // 500 * 10ms = 5 seconds.

    while ((lastDirPath.empty() || lastbRecursive.empty()) && counter < 500)
    {
        std::this_thread::sleep_for(std::chrono::milliseconds(10));
        counter++;
    }

    BOOST_ASSERT_MSG(counter != 500, "Websocket took too long to respond");

    BOOST_CHECK_EQUAL(lastDirPath.size(), 1);
    BOOST_CHECK_EQUAL(lastDirPath.back(), "");
    BOOST_CHECK_EQUAL(lastbRecursive.back(), true);

    // The file list caching can take a moment to complete in the background
    // Check that the correct file list cache entries were added to the database

    for (counter = 0; counter < 500; counter++)
    {
        auto fileListCacheResult = database->run(
            select(all_of(jobFilelistcache)).from(jobFilelistcache).where(jobFilelistcache.jobId == jobId));

        // Has the file list cache finished being populated by the background system thread?
        if (!fileListCacheResult.empty())
        {
            break;
        }


        std::this_thread::sleep_for(std::chrono::milliseconds(10));
    }

    // Fail if counter = 500
    BOOST_ASSERT_MSG(counter != 500, "Background file list caching thread failed to run correctly");

    // Reset the websocket trackers
    lastDirPath.clear();
    lastbRecursive.clear();

    // This file list will be complete, and should read files from only the file cache
    response = httpClient.request("PATCH",
                                  "/job/apiv1/file/",
                                  jsonParams.dump(),
                                  {
                                      {"Authorization", jwtToken.signature()}
    });

    // There should have been no websocket calls - since all file information should have been read from the cache
    BOOST_CHECK_EQUAL(lastDirPath.size(), 0);

    response->content >> jsonResult;
    BOOST_CHECK(jsonResult.contains("files"));
    jsonData = jsonResult["files"];

    // Check that the file list returned was correct
    for (auto index = 0; index < expected.size(); index++)
    {
        const auto& file = expected[index];

        BOOST_CHECK_EQUAL(jsonData[index]["path"], file.fileName);
        BOOST_CHECK_EQUAL(jsonData[index]["isDir"], file.isDirectory);
        BOOST_CHECK_EQUAL(jsonData[index]["fileSize"], file.fileSize);
        BOOST_CHECK_EQUAL(jsonData[index]["permissions"], file.permissions);
    }

    // Check that the correct file list cache entries were added to the database
    auto fileListCacheResult =
        database->run(select(all_of(jobFilelistcache)).from(jobFilelistcache).where(jobFilelistcache.jobId == jobId));

    BOOST_CHECK_EQUAL(fileListCacheResult.empty(), false);

    std::vector<sFile> files;
    for (const auto& file : fileListCacheResult)
    {
        files.push_back({.fileName    = file.path,
                         .fileSize    = static_cast<uint64_t>(file.fileSize),
                         .permissions = static_cast<uint32_t>(file.permissions),
                         .isDirectory = static_cast<bool>(file.isDir)});
    }

    for (auto i = 0; i < files.size(); i++)
    {
        BOOST_CHECK_EQUAL(files[i].fileName, fileListData[i].fileName);
        BOOST_CHECK_EQUAL(files[i].isDirectory, fileListData[i].isDirectory);
        BOOST_CHECK_EQUAL(files[i].fileSize, fileListData[i].fileSize);
        BOOST_CHECK_EQUAL(files[i].permissions, fileListData[i].permissions);
    }

    // Reset the websocket trackers
    lastDirPath.clear();
    lastbRecursive.clear();

    // This file list will be complete, and should not call the websocket since all files should be cached
    response = httpClient.request("PATCH",
                                  "/job/apiv1/file/",
                                  jsonParams.dump(),
                                  {
                                      {"Authorization", jwtToken.signature()}
    });

    // There should have been no websocket calls
    BOOST_CHECK_EQUAL(lastDirPath.size(), 0);

    response->content >> jsonResult;
    BOOST_CHECK(jsonResult.contains("files"));
    jsonData = jsonResult["files"];

    // Check that the file list returned was correct
    for (auto index = 0; index < expected.size(); index++)
    {
        const auto& file = expected[index];

        BOOST_CHECK_EQUAL(jsonData[index]["path"], file.fileName);
        BOOST_CHECK_EQUAL(jsonData[index]["isDir"], file.isDirectory);
        BOOST_CHECK_EQUAL(jsonData[index]["fileSize"], file.fileSize);
        BOOST_CHECK_EQUAL(jsonData[index]["permissions"], file.permissions);
    }

    jsonParams = {
        {    "jobId", jobId},
        {"recursive",  true},
        {     "path",   "/"}
    };

    // This file list will be complete, and should not call the websocket since all files should be cached
    response = httpClient.request("PATCH",
                                  "/job/apiv1/file/",
                                  jsonParams.dump(),
                                  {
                                      {"Authorization", jwtToken.signature()}
    });

    // There should have been no websocket calls
    BOOST_CHECK_EQUAL(lastDirPath.size(), 0);

    response->content >> jsonResult;
    BOOST_CHECK(jsonResult.contains("files"));
    jsonData = jsonResult["files"];

    // Check that the file list returned was correct
    for (auto index = 0; index < fileListData.size(); index++)
    {
        auto file = fileListData[index];

        BOOST_CHECK_EQUAL(jsonData[index]["path"], file.fileName);
        BOOST_CHECK_EQUAL(jsonData[index]["isDir"], file.isDirectory);
        BOOST_CHECK_EQUAL(jsonData[index]["fileSize"], file.fileSize);
        BOOST_CHECK_EQUAL(jsonData[index]["permissions"], file.permissions);
    }

    jsonParams = {
        {    "jobId",      jobId},
        {"recursive",       true},
        {     "path", "/testdir"}
    };

    // This file list will be complete, and should not call the websocket since all files should be cached
    response = httpClient.request("PATCH",
                                  "/job/apiv1/file/",
                                  jsonParams.dump(),
                                  {
                                      {"Authorization", jwtToken.signature()}
    });

    // There should have been no websocket calls
    BOOST_CHECK_EQUAL(lastDirPath.size(), 0);

    response->content >> jsonResult;
    BOOST_CHECK(jsonResult.contains("files"));
    jsonData = jsonResult["files"];


    expected = {fileListData[2], fileListData[3], fileListData[4], fileListData[5], fileListData[6], fileListData[7]};


    // Check that the file list returned was correct
    for (auto index = 0; index < expected.size(); index++)
    {
        const auto& file = expected[index];

        BOOST_CHECK_EQUAL(jsonData[index]["path"], file.fileName);
        BOOST_CHECK_EQUAL(jsonData[index]["isDir"], file.isDirectory);
        BOOST_CHECK_EQUAL(jsonData[index]["fileSize"], file.fileSize);
        BOOST_CHECK_EQUAL(jsonData[index]["permissions"], file.permissions);
    }
}

BOOST_AUTO_TEST_CASE(test_file_list_no_jobid)
{
    setJwtSecret(std::static_pointer_cast<HttpServer>(httpServer)->getvJwtSecrets()->at(1).secret());

    // The jobId received by the websocket should always be 0 in this test
    jobId = 0;

    // Create jsonParams
    jsonParams = {
        {  "cluster", std::static_pointer_cast<HttpServer>(httpServer)->getvJwtSecrets()->at(2).clusters()[0]},
        {   "bundle",                                                                        "my_test_bundle"},
        {"recursive",                                                                                   false},
        {     "path",                                                                              "/testdir"}
    };

    // This call should cause an error to be raised and reported
    auto response = httpClient.request("PATCH",
                                       "/job/apiv1/file/",
                                       jsonParams.dump(),
                                       {
                                           {"Authorization", jwtToken.signature()}
    });
    BOOST_CHECK_EQUAL(std::stoi(response->status_code), (int)SimpleWeb::StatusCode::client_error_bad_request);
    BOOST_CHECK_EQUAL(response->content.string(), "test error");
    auto fileListMap3 = application->getFileListMap();
    BOOST_CHECK_EQUAL(fileListMap3->size(), 0);

    // Finish error testing
    bRaiseError = false;

    // This file list will be complete, but won't create any file cache objects because the job has no
    // _job_completion_ job history objects
    response = httpClient.request("PATCH",
                                  "/job/apiv1/file/",
                                  jsonParams.dump(),
                                  {
                                      {"Authorization", jwtToken.signature()}
    });

    auto fileListMap4 = application->getFileListMap();
    BOOST_CHECK_EQUAL(fileListMap4->size(), 0);

    BOOST_CHECK_EQUAL(lastDirPath.size(), 1);
    BOOST_CHECK_EQUAL(lastDirPath.back(), std::string(jsonParams["path"]));
    BOOST_CHECK_EQUAL(lastbRecursive.back(), (bool)jsonParams["recursive"]);


    std::vector<sFile> expected = {fileListData[2], fileListData[3], fileListData[4], fileListData[5], fileListData[6]};


    response->content >> jsonResult;
    BOOST_CHECK(jsonResult.contains("files"));
    auto jsonData = jsonResult["files"];

    // Check that the file list returned was correct
    for (auto index = 0; index < expected.size(); index++)
    {
        const auto& file = expected[index];

        BOOST_CHECK_EQUAL(jsonData[index]["path"], file.fileName);
        BOOST_CHECK_EQUAL(jsonData[index]["isDir"], file.isDirectory);
        BOOST_CHECK_EQUAL(jsonData[index]["fileSize"], file.fileSize);
        BOOST_CHECK_EQUAL(jsonData[index]["permissions"], file.permissions);
    }
}
BOOST_AUTO_TEST_SUITE_END()
