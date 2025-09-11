//
// Created by lewis on 22/10/20.
//
import job_status;
#include "../../tests/fixtures/DatabaseFixture.h"
#include "../../tests/fixtures/HttpClientFixture.h"
#include "../../tests/fixtures/HttpServerFixture.h"
#include <boost/test/unit_test.hpp>


struct FileTestDataFixture : public DatabaseFixture, public HttpServerFixture, public HttpClientFixture {
    uint64_t jobId1;
    uint64_t jobId2;
    uint64_t jobId3;

    FileTestDataFixture() {
        // Fabricate data
        // Create the new job object
        jobId1 = database->run(
                insert_into(jobTable)
                        .set(
                                jobTable.user = 1,
                                jobTable.parameters = "params1",
                                jobTable.cluster = httpServer->getvJwtSecrets()->at(0).clusters()[0],
                                jobTable.bundle = "whatever",
                                jobTable.application = httpServer->getvJwtSecrets()->at(0).name()
                        )
        );

        // Create the first state object
        database->run(
                insert_into(jobHistoryTable)
                        .set(
                                jobHistoryTable.jobId = jobId1,
                                jobHistoryTable.timestamp = std::chrono::system_clock::now(),
                                jobHistoryTable.what = SYSTEM_SOURCE,
                                jobHistoryTable.state = static_cast<uint32_t>(JobStatus::PENDING),
                                jobHistoryTable.details = "Job submitting"
                        )
        );

        // Create the new job object
        jobId2 = database->run(
                insert_into(jobTable)
                        .set(
                                jobTable.user = 1,
                                jobTable.parameters = "params1",
                                jobTable.cluster = httpServer->getvJwtSecrets()->at(0).clusters()[0],
                                jobTable.bundle = "whatever",
                                jobTable.application = httpServer->getvJwtSecrets()->at(0).name()
                        )
        );

        // Create the first state object
        database->run(
                insert_into(jobHistoryTable)
                        .set(
                                jobHistoryTable.jobId = jobId2,
                                jobHistoryTable.timestamp = std::chrono::system_clock::now(),
                                jobHistoryTable.what = SYSTEM_SOURCE,
                                jobHistoryTable.state = static_cast<uint32_t>(JobStatus::PENDING),
                                jobHistoryTable.details = "Job submitting"
                        )
        );

        // Create the new job object
        jobId3 = database->run(
                insert_into(jobTable)
                        .set(
                                jobTable.user = 1,
                                jobTable.parameters = "params1",
                                jobTable.cluster = httpServer->getvJwtSecrets()->at(1).clusters()[0],
                                jobTable.bundle = "whatever",
                                jobTable.application = httpServer->getvJwtSecrets()->at(1).name()
                        )
        );

        // Create the first state object
        database->run(
                insert_into(jobHistoryTable)
                        .set(
                                jobHistoryTable.jobId = jobId3,
                                jobHistoryTable.timestamp = std::chrono::system_clock::now(),
                                jobHistoryTable.what = SYSTEM_SOURCE,
                                jobHistoryTable.state = static_cast<uint32_t>(JobStatus::PENDING),
                                jobHistoryTable.details = "Job submitting"
                        )
        );
    }
};

BOOST_FIXTURE_TEST_SUITE(File_test_suite, FileTestDataFixture)
/*
 * This test suite is responsible for testing the File HTTP Rest API
 */

    BOOST_AUTO_TEST_CASE(create_download_unauthorized) {
        // Test unauthorized user
        auto result = httpClient.request("POST", "/job/apiv1/file/", "", {{"Authorization", "not_valid"}});
        BOOST_CHECK_EQUAL(result->content.string(), "Not authorized");

    }

    BOOST_AUTO_TEST_CASE(create_download_invalid_payload) {
        setJwtSecret(httpServer->getvJwtSecrets()->at(0).secret());

        // Test creating a file download with invalid payload but authorized user
        auto result = httpClient.request("POST", "/job/apiv1/file/", "", {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(result->content.string(), "Bad request");
        BOOST_CHECK_EQUAL(std::stoi(result->status_code), (int) SimpleWeb::StatusCode::client_error_bad_request);
    }

    BOOST_AUTO_TEST_CASE(create_download_job1_success) {
        // Test creating a file download for job1 - should be successful because job1 was created by app1 making this request
        setJwtSecret(httpServer->getvJwtSecrets()->at(0).secret());

        jsonParams = {
                {"jobId", jobId1},
                {"path",  "/an_awesome_path/"}
        };

        auto result = httpClient.request("POST", "/job/apiv1/file/", jsonParams.dump(),
                                         {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(std::stoi(result->status_code), (int) SimpleWeb::StatusCode::success_ok);
        BOOST_CHECK_EQUAL(result->header.find("Content-Type")->second, "application/json");

        result->content >> jsonResult;

        BOOST_CHECK_MESSAGE(jsonResult.find("fileId") != jsonResult.end(),
                            "result.find(\"fileId\") != result.end() was not the expected value");
    }

    BOOST_AUTO_TEST_CASE(create_download_job1_no_jobid_invalid_payload) {
        setJwtSecret(httpServer->getvJwtSecrets()->at(0).secret());

        // Test that creating a file download without specifying a jobId, bundle, or cluster fails
        jsonParams = {
                {"path",  "/an_awesome_path/"}
        };

        auto result = httpClient.request("POST", "/job/apiv1/file/", jsonParams.dump(), {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(result->content.string(), "Bad request");
        BOOST_CHECK_EQUAL(std::stoi(result->status_code), (int) SimpleWeb::StatusCode::client_error_bad_request);

        // Test that creating a file download without specifying a jobId, or cluster fails
        jsonParams = {
                {"bundle", "test_bundle"},
                {"path",  "/an_awesome_path/"}
        };

        result = httpClient.request("POST", "/job/apiv1/file/", jsonParams.dump(), {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(result->content.string(), "Bad request");
        BOOST_CHECK_EQUAL(std::stoi(result->status_code), (int) SimpleWeb::StatusCode::client_error_bad_request);

        // Test that creating a file download without specifying a jobId, or bundle fails
        jsonParams = {
                {"cluster", "test_cluster"},
                {"path",  "/an_awesome_path/"}
        };

        result = httpClient.request("POST", "/job/apiv1/file/", jsonParams.dump(), {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(result->content.string(), "Bad request");
        BOOST_CHECK_EQUAL(std::stoi(result->status_code), (int) SimpleWeb::StatusCode::client_error_bad_request);
    }

    BOOST_AUTO_TEST_CASE(create_download_job1_no_jobid_success) {
        // Test that creating a file download without a jobId works correctly
        setJwtSecret(httpServer->getvJwtSecrets()->at(0).secret());

        // No jobId key
        jsonParams = {
                {"bundle", "test_bundle"},
                {"cluster", httpServer->getvJwtSecrets()->at(0).clusters()[0]},
                {"path",  "/an_awesome_path/"}
        };

        auto result = httpClient.request("POST", "/job/apiv1/file/", jsonParams.dump(),
                                         {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(std::stoi(result->status_code), (int) SimpleWeb::StatusCode::success_ok);
        BOOST_CHECK_EQUAL(result->header.find("Content-Type")->second, "application/json");

        result->content >> jsonResult;

        BOOST_CHECK_MESSAGE(jsonResult.find("fileId") != jsonResult.end(),
                            "result.find(\"fileId\") != result.end() was not the expected value");

        // jobId key with value 0
        jsonParams = {
                {"jobId", 0},
                {"bundle", "test_bundle"},
                {"cluster", httpServer->getvJwtSecrets()->at(0).clusters()[0]},
                {"path",  "/an_awesome_path/"}
        };

        result = httpClient.request("POST", "/job/apiv1/file/", jsonParams.dump(),
                                    {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(std::stoi(result->status_code), (int) SimpleWeb::StatusCode::success_ok);
        BOOST_CHECK_EQUAL(result->header.find("Content-Type")->second, "application/json");

        result->content >> jsonResult;

        BOOST_CHECK_MESSAGE(jsonResult.find("fileId") != jsonResult.end(),
                            "result.find(\"fileId\") != result.end() was not the expected value");
    }

    BOOST_AUTO_TEST_CASE(create_download_job1_no_jobid_no_cluster_access) {
        // Test that creating a file download without a jobId referencing a cluster that the secret doesn't have access
        // to fails
        setJwtSecret(httpServer->getvJwtSecrets()->at(3).secret());

        // No jobId key
        jsonParams = {
                {"bundle",  "test_bundle"},
                {"cluster", httpServer->getvJwtSecrets()->at(0).clusters()[0]},
                {"path",    "/an_awesome_path/"}
        };

        auto result = httpClient.request("POST", "/job/apiv1/file/", jsonParams.dump(), {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(result->content.string(), "Bad request");
        BOOST_CHECK_EQUAL(std::stoi(result->status_code), (int) SimpleWeb::StatusCode::client_error_bad_request);
    }

    BOOST_AUTO_TEST_CASE(create_download_job1_no_jobid_invalid_cluster) {
        // Test that creating a file download without a jobId referencing a cluster that the secret doesn't have access
        // to fails
        setJwtSecret(httpServer->getvJwtSecrets()->at(3).secret());

        // No jobId key
        jsonParams = {
                {"bundle",  "test_bundle"},
                {"cluster", "not_a_real_cluster"},
                {"path",    "/an_awesome_path/"}
        };

        auto result = httpClient.request("POST", "/job/apiv1/file/", jsonParams.dump(), {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(result->content.string(), "Bad request");
        BOOST_CHECK_EQUAL(std::stoi(result->status_code), (int) SimpleWeb::StatusCode::client_error_bad_request);
    }

    BOOST_AUTO_TEST_CASE(create_download_app2_can_access_app1) {
        // Check that app2 can request a file download for a job run by app1
        setJwtSecret(httpServer->getvJwtSecrets()->at(1).secret());

        jsonParams = {
                {"jobId", jobId1},
                {"path",  "/an_awesome_path/"}
        };

        auto result = httpClient.request("POST", "/job/apiv1/file/", jsonParams.dump(),
                                    {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(std::stoi(result->status_code), (int) SimpleWeb::StatusCode::success_ok);
        BOOST_CHECK_EQUAL(result->header.find("Content-Type")->second, "application/json");

        result->content >> jsonResult;

        BOOST_CHECK_MESSAGE(jsonResult.find("fileId") != jsonResult.end(),
                            "result.find(\"fileId\") != result.end() was not the expected value");
    }

    BOOST_AUTO_TEST_CASE(create_download_app4_cant_access_app1) {
        // Check that app4 can't request a file download for a job run by app1
        setJwtSecret(httpServer->getvJwtSecrets()->at(3).secret());

        jsonParams = {
                {"jobId", jobId1},
                {"path",  "/an_awesome_path/"}
        };

        auto result = httpClient.request("POST", "/job/apiv1/file/", jsonParams.dump(),
                                    {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(result->content.string(), "Bad request");
        BOOST_CHECK_EQUAL(std::stoi(result->status_code), (int) SimpleWeb::StatusCode::client_error_bad_request);

    }

    BOOST_AUTO_TEST_CASE(create_download_multiple) {
        // Test creating multiple file downloads for job1
        setJwtSecret(httpServer->getvJwtSecrets()->at(0).secret());

        jsonParams = {
                {"jobId", jobId1},
                {"paths",
                          {
                                  "/an_awesome_path1/",
                                  "/an_awesome_path2/",
                                  "/an_awesome_path3/",
                                  "/an_awesome_path4/",
                                  "/an_awesome_path5/"
                          }
                }
        };

        auto result = httpClient.request("POST", "/job/apiv1/file/", jsonParams.dump(),
                                    {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(std::stoi(result->status_code), (int) SimpleWeb::StatusCode::success_ok);
        BOOST_CHECK_EQUAL(result->header.find("Content-Type")->second, "application/json");

        result->content >> jsonResult;

        BOOST_CHECK_MESSAGE(jsonResult.find("fileIds") != jsonResult.end(),
                            "result.find(\"fileIds\") != result.end() was not the expected value");

        BOOST_CHECK_MESSAGE(jsonResult["fileIds"].size() == 5,
                            "result[\"fileIds\"].size() == 5 was not the expected value");
    }

    BOOST_AUTO_TEST_CASE(create_download_empty_path_list_no_exception) {
        // Test empty file path list doesn't cause an exception
        setJwtSecret(httpServer->getvJwtSecrets()->at(0).secret());

        jsonParams = {
                {"jobId", jobId1},
                {"paths", std::vector<std::string>()}
        };

        auto result = httpClient.request("POST", "/job/apiv1/file/", jsonParams.dump(), {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(std::stoi(result->status_code), (int) SimpleWeb::StatusCode::success_ok);
        BOOST_CHECK_EQUAL(result->header.find("Content-Type")->second, "application/json");

        result->content >> jsonResult;

        BOOST_CHECK_MESSAGE(jsonResult.find("fileIds") != jsonResult.end(),
                            "result.find(\"fileIds\") != result.end() was not the expected value");

        BOOST_CHECK_MESSAGE(jsonResult["fileIds"].empty(),
                            "result[\"fileIds\"].size() == 5 was not the expected value");
    }

    BOOST_AUTO_TEST_CASE(get_file_list_unauthorized) {
        // Test unauthorized user
        auto result = httpClient.request("PATCH", "/job/apiv1/file/", "", {{"Authorization", "not_valid"}});
        BOOST_CHECK_EQUAL(result->content.string(), "Not authorized");
    }

    BOOST_AUTO_TEST_CASE(get_file_list_invalid_payload) {
        setJwtSecret(httpServer->getvJwtSecrets()->at(0).secret());

        // Test requesting a file list with invalid payload but authorized user
        auto result = httpClient.request("PATCH", "/job/apiv1/file/", "", {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(result->content.string(), "Bad request");
        BOOST_CHECK_EQUAL(std::stoi(result->status_code), (int) SimpleWeb::StatusCode::client_error_bad_request);
    }

    BOOST_AUTO_TEST_CASE(get_file_list_job1_success) {
        // Test requesting a file list for job1 - should be successful because job1 was created by app1 making this request
        setJwtSecret(httpServer->getvJwtSecrets()->at(0).secret());
        
        jsonParams = {
                {"jobId",     jobId1},
                {"recursive", true},
                {"path",      "/an_awesome_path/"}
        };

        auto result = httpClient.request("PATCH", "/job/apiv1/file/", jsonParams.dump(),
                                    {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(std::stoi(result->status_code),
                          (int) SimpleWeb::StatusCode::server_error_service_unavailable);
        BOOST_CHECK_EQUAL(result->content.string(), "Remote Cluster Offline");
    }

    BOOST_AUTO_TEST_CASE(get_file_list_app2_can_access_app1) {
        // Check that app2 can request a file list for a job run by app1
        setJwtSecret(httpServer->getvJwtSecrets()->at(1).secret());

        jsonParams = {
                {"jobId",     jobId1},
                {"recursive", true},
                {"path",      "/an_awesome_path/"}
        };

        auto result = httpClient.request("PATCH", "/job/apiv1/file/", jsonParams.dump(),
                                    {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(std::stoi(result->status_code),
                          (int) SimpleWeb::StatusCode::server_error_service_unavailable);
        BOOST_CHECK_EQUAL(result->content.string(), "Remote Cluster Offline");
    }

    BOOST_AUTO_TEST_CASE(get_file_list_app4_cant_access_app1) {
        // Check that app4 can't request a file list for a job run by app1
        setJwtSecret(httpServer->getvJwtSecrets()->at(3).secret());

        jsonParams = {
                {"jobId",     jobId1},
                {"recursive", true},
                {"path",      "/an_awesome_path/"}
        };

        auto result = httpClient.request("PATCH", "/job/apiv1/file/", jsonParams.dump(), {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(result->content.string(), "Bad request");
        BOOST_CHECK_EQUAL(std::stoi(result->status_code), (int) SimpleWeb::StatusCode::client_error_bad_request);
    }

    BOOST_AUTO_TEST_CASE(get_file_list_job1_no_jobid_invalid_payload) {
        setJwtSecret(httpServer->getvJwtSecrets()->at(0).secret());

        // Test that getting a file list without specifying a jobId, bundle, or cluster fails
        jsonParams = {
                {"recursive", true},
                {"path",      "/an_awesome_path/"}
        };

        auto result = httpClient.request("PATCH", "/job/apiv1/file/", jsonParams.dump(), {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(result->content.string(), "Bad request");
        BOOST_CHECK_EQUAL(std::stoi(result->status_code), (int) SimpleWeb::StatusCode::client_error_bad_request);

        // Test that getting a file list without specifying a jobId, or cluster fails
        jsonParams = {
                {"bundle", "test_bundle"},
                {"recursive", true},
                {"path",  "/an_awesome_path/"}
        };

        result = httpClient.request("PATCH", "/job/apiv1/file/", jsonParams.dump(), {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(result->content.string(), "Bad request");
        BOOST_CHECK_EQUAL(std::stoi(result->status_code), (int) SimpleWeb::StatusCode::client_error_bad_request);

        // Test that getting a file list without specifying a jobId, or bundle fails
        jsonParams = {
                {"cluster", "test_cluster"},
                {"recursive", true},
                {"path",  "/an_awesome_path/"}
        };

        result = httpClient.request("PATCH", "/job/apiv1/file/", jsonParams.dump(), {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(result->content.string(), "Bad request");
        BOOST_CHECK_EQUAL(std::stoi(result->status_code), (int) SimpleWeb::StatusCode::client_error_bad_request);
    }

    BOOST_AUTO_TEST_CASE(get_file_list_job1_no_jobid_success) {
        // Test that creating a file download without a jobId works correctly
        setJwtSecret(httpServer->getvJwtSecrets()->at(0).secret());

        // No jobId key
        jsonParams = {
                {"bundle", "test_bundle"},
                {"cluster", httpServer->getvJwtSecrets()->at(0).clusters()[0]},
                {"recursive", true},
                {"path",  "/an_awesome_path/"}
        };

        auto result = httpClient.request("PATCH", "/job/apiv1/file/", jsonParams.dump(),
                                         {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(std::stoi(result->status_code),
                          (int) SimpleWeb::StatusCode::server_error_service_unavailable);
        BOOST_CHECK_EQUAL(result->content.string(), "Remote Cluster Offline");

        // jobId key with value 0
        jsonParams = {
                {"jobId", 0},
                {"bundle", "test_bundle"},
                {"cluster", httpServer->getvJwtSecrets()->at(0).clusters()[0]},
                {"recursive", true},
                {"path",  "/an_awesome_path/"}
        };

        result = httpClient.request("PATCH", "/job/apiv1/file/", jsonParams.dump(),
                                         {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(std::stoi(result->status_code),
                          (int) SimpleWeb::StatusCode::server_error_service_unavailable);
        BOOST_CHECK_EQUAL(result->content.string(), "Remote Cluster Offline");
    }

    BOOST_AUTO_TEST_CASE(get_file_list_job1_no_jobid_no_cluster_access) {
        // Test that creating a file download without a jobId referencing a cluster that the secret doesn't have access
        // to fails
        setJwtSecret(httpServer->getvJwtSecrets()->at(3).secret());

        // No jobId key
        jsonParams = {
                {"bundle",  "test_bundle"},
                {"cluster", httpServer->getvJwtSecrets()->at(0).clusters()[0]},
                {"recursive", true},
                {"path",    "/an_awesome_path/"}
        };

        auto result = httpClient.request("PATCH", "/job/apiv1/file/", jsonParams.dump(), {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(result->content.string(), "Bad request");
        BOOST_CHECK_EQUAL(std::stoi(result->status_code), (int) SimpleWeb::StatusCode::client_error_bad_request);
    }

    BOOST_AUTO_TEST_CASE(get_file_list_job1_no_jobid_invalid_cluster) {
        // Test that creating a file download without a jobId referencing a cluster that the secret doesn't have access
        // to fails
        setJwtSecret(httpServer->getvJwtSecrets()->at(3).secret());

        // No jobId key
        jsonParams = {
                {"bundle",  "test_bundle"},
                {"cluster", "not_a_real_cluster"},
                {"recursive", true},
                {"path",    "/an_awesome_path/"}
        };

        auto result = httpClient.request("PATCH", "/job/apiv1/file/", jsonParams.dump(), {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(result->content.string(), "Bad request");
        BOOST_CHECK_EQUAL(std::stoi(result->status_code), (int) SimpleWeb::StatusCode::client_error_bad_request);
    }
BOOST_AUTO_TEST_SUITE_END()