//
// Created by lewis on 22/10/20.
//
#include <boost/test/unit_test.hpp>
#include <client_http.hpp>
#include <jwt/jwt.hpp>
#include "../../Settings.h"
#include "../../Cluster/ClusterManager.h"
#include "../HttpServer.h"
#include "../../DB/MySqlConnector.h"
#include "../../Lib/jobserver_schema.h"
#include "../../Lib/JobStatus.h"

using HttpClient = SimpleWeb::Client<SimpleWeb::HTTP>;

BOOST_AUTO_TEST_SUITE(File_test_suite)
/*
 * This test suite is responsible for testing the File HTTP Rest API
 */

    auto sAccess = R"(
    [
        {
            "name": "app1",
            "secret": "super_secret1",
            "applications": [],
            "clusters": [
                "cluster2",
                "cluster3"
            ]
        },
        {
            "name": "app2",
            "secret": "super_secret2",
            "applications": [
                "app1"
            ],
            "clusters": [
                "cluster1"
            ]
        },
        {
            "name": "app3",
            "secret": "super_secret3",
            "applications": [
                "app1",
                "app2"
            ],
            "clusters": [
                "cluster1",
                "cluster2",
                "cluster3"
            ]
        },
        {
            "name": "app4",
            "secret": "super_secret4",
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
        },
        {
            "name": "cluster2",
            "host": "cluster2.com",
            "username": "user2",
            "path": "/cluster2/",
            "key": "cluster2_key"
        },
        {
            "name": "cluster3",
            "host": "cluster3.com",
            "username": "user3",
            "path": "/cluster3/",
            "key": "cluster3_key"
        }
    ]
    )";

    BOOST_AUTO_TEST_CASE(test_POST_create_download) {
        // Delete all file download tokens just in case
        auto db = MySqlConnector();
        schema::JobserverFiledownload fileDownloadTable;
        schema::JobserverJob jobTable;
        schema::JobserverJobhistory jobHistoryTable;
        db->run(remove_from(fileDownloadTable).unconditionally());
        db->run(remove_from(jobHistoryTable).unconditionally());
        db->run(remove_from(jobTable).unconditionally());

        // Set up the test server
        setenv(CLUSTER_CONFIG_ENV_VARIABLE, base64Encode(sClusters).c_str(), 1);
        auto mgr = ClusterManager();

        setenv(ACCESS_SECRET_ENV_VARIABLE, base64Encode(sAccess).c_str(), 1);
        auto svr = HttpServer(&mgr);

        svr.start();
        std::this_thread::sleep_for(std::chrono::seconds(1));

        // Fabricate data
        // Create the new job object
        auto jobId1 = db->run(
                insert_into(jobTable)
                        .set(
                                jobTable.user = 1,
                                jobTable.parameters = "params1",
                                jobTable.cluster = svr.getvJwtSecrets()->at(0).clusters()[0],
                                jobTable.bundle = "whatever",
                                jobTable.application = svr.getvJwtSecrets()->at(0).name()
                        )
        );

        // Create the first state object
        db->run(
                insert_into(jobHistoryTable)
                        .set(
                                jobHistoryTable.jobId = jobId1,
                                jobHistoryTable.timestamp = std::chrono::system_clock::now(),
                                jobHistoryTable.what = "system",
                                jobHistoryTable.state = (uint32_t) JobStatus::PENDING,
                                jobHistoryTable.details = "Job submitting"
                        )
        );

        // Create the new job object
        auto jobId2 = db->run(
                insert_into(jobTable)
                        .set(
                                jobTable.user = 1,
                                jobTable.parameters = "params1",
                                jobTable.cluster = svr.getvJwtSecrets()->at(0).clusters()[0],
                                jobTable.bundle = "whatever",
                                jobTable.application = svr.getvJwtSecrets()->at(0).name()
                        )
        );

        // Create the first state object
        db->run(
                insert_into(jobHistoryTable)
                        .set(
                                jobHistoryTable.jobId = jobId2,
                                jobHistoryTable.timestamp = std::chrono::system_clock::now(),
                                jobHistoryTable.what = "system",
                                jobHistoryTable.state = (uint32_t) JobStatus::PENDING,
                                jobHistoryTable.details = "Job submitting"
                        )
        );

        // Create the new job object
        auto jobId3 = db->run(
                insert_into(jobTable)
                        .set(
                                jobTable.user = 1,
                                jobTable.parameters = "params1",
                                jobTable.cluster = svr.getvJwtSecrets()->at(1).clusters()[0],
                                jobTable.bundle = "whatever",
                                jobTable.application = svr.getvJwtSecrets()->at(1).name()
                        )
        );

        // Create the first state object
        db->run(
                insert_into(jobHistoryTable)
                        .set(
                                jobHistoryTable.jobId = jobId3,
                                jobHistoryTable.timestamp = std::chrono::system_clock::now(),
                                jobHistoryTable.what = "system",
                                jobHistoryTable.state = (uint32_t) JobStatus::PENDING,
                                jobHistoryTable.details = "Job submitting"
                        )
        );

        // Set up the test client
        HttpClient client("localhost:8000");

        // Test unauthorized user
        auto r = client.request("POST", "/job/apiv1/file/", "", {{"Authorization", "not_valid"}});
        BOOST_CHECK_EQUAL(r->content.string(), "Not authorized");

        auto now = std::chrono::system_clock::now() + std::chrono::seconds{10};
        jwt::jwt_object jwtToken = {
                jwt::params::algorithm("HS256"),
                jwt::params::payload({{"userName", "User"}}),
                jwt::params::secret(svr.getvJwtSecrets()->at(0).secret())
        };
        jwtToken.add_claim("exp", now);

        // Since payload above only accepts string values, we need to set up any non-string values
        // separately
        jwtToken.payload().add_claim("userId", 5);

        // Test creating a file download with invalid payload but authorized user
        r = client.request("POST", "/job/apiv1/file/", "", {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(r->content.string(), "Bad request");
        BOOST_CHECK_EQUAL(std::stoi(r->status_code), (int) SimpleWeb::StatusCode::client_error_bad_request);

        // Test creating a file download with invalid payload but authorized user
        r = client.request("POST", "/job/apiv1/file/", "", {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(r->content.string(), "Bad request");
        BOOST_CHECK_EQUAL(std::stoi(r->status_code), (int) SimpleWeb::StatusCode::client_error_bad_request);

        // Test creating a file download for job1 - should be successful because job1 was created by app1 making this request
        nlohmann::json params = {
                {"jobId", jobId1},
                {"path",  "/an_awesome_path/"}
        };

        r = client.request("POST", "/job/apiv1/file/", params.dump(), {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(std::stoi(r->status_code), (int) SimpleWeb::StatusCode::success_ok);
        BOOST_CHECK_EQUAL(r->header.find("Content-Type")->second, "application/json");

        nlohmann::json result;
        r->content >> result;

        BOOST_CHECK_MESSAGE(result.find("fileId") != result.end(),
                            "result.find(\"fileId\") != result.end() was not the expected value");

        // Check that app2 can request a file download for a job run by app1
        jwtToken = {
                jwt::params::algorithm("HS256"),
                jwt::params::payload({{"userName", "User"}}),
                jwt::params::secret(svr.getvJwtSecrets()->at(1).secret())
        };
        jwtToken.add_claim("exp", now);

        // Since payload above only accepts string values, we need to set up any non-string values
        // separately
        jwtToken.payload().add_claim("userId", 5);

        r = client.request("POST", "/job/apiv1/file/", params.dump(), {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(std::stoi(r->status_code), (int) SimpleWeb::StatusCode::success_ok);
        BOOST_CHECK_EQUAL(r->header.find("Content-Type")->second, "application/json");

        r->content >> result;

        BOOST_CHECK_MESSAGE(result.find("fileId") != result.end(),
                            "result.find(\"fileId\") != result.end() was not the expected value");

        // Check that app4 can't request a file download for a job run by app1
        jwtToken = {
                jwt::params::algorithm("HS256"),
                jwt::params::payload({{"userName", "User"}}),
                jwt::params::secret(svr.getvJwtSecrets()->at(3).secret())
        };
        jwtToken.add_claim("exp", now);

        // Since payload above only accepts string values, we need to set up any non-string values
        // separately
        jwtToken.payload().add_claim("userId", 5);

        r = client.request("POST", "/job/apiv1/file/", params.dump(), {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(r->content.string(), "Bad request");
        BOOST_CHECK_EQUAL(std::stoi(r->status_code), (int) SimpleWeb::StatusCode::client_error_bad_request);

        // Finished with the server
        svr.stop();

        // Clean up
        db->run(remove_from(fileDownloadTable).unconditionally());
        db->run(remove_from(jobHistoryTable).unconditionally());
        db->run(remove_from(jobTable).unconditionally());
    }

    BOOST_AUTO_TEST_CASE(test_PATCH_get_file_list) {
        // Delete all file download tokens just in case
        auto db = MySqlConnector();
        schema::JobserverFiledownload fileDownloadTable;
        schema::JobserverJob jobTable;
        schema::JobserverJobhistory jobHistoryTable;
        db->run(remove_from(fileDownloadTable).unconditionally());
        db->run(remove_from(jobHistoryTable).unconditionally());
        db->run(remove_from(jobTable).unconditionally());

        // Set up the test server
        setenv(CLUSTER_CONFIG_ENV_VARIABLE, base64Encode(sClusters).c_str(), 1);
        auto mgr = ClusterManager();

        setenv(ACCESS_SECRET_ENV_VARIABLE, base64Encode(sAccess).c_str(), 1);
        auto svr = HttpServer(&mgr);

        svr.start();
        std::this_thread::sleep_for(std::chrono::seconds(1));

        // Fabricate data
        // Create the new job object
        auto jobId1 = db->run(
                insert_into(jobTable)
                        .set(
                                jobTable.user = 1,
                                jobTable.parameters = "params1",
                                jobTable.cluster = svr.getvJwtSecrets()->at(0).clusters()[0],
                                jobTable.bundle = "whatever",
                                jobTable.application = svr.getvJwtSecrets()->at(0).name()
                        )
        );

        // Create the first state object
        db->run(
                insert_into(jobHistoryTable)
                        .set(
                                jobHistoryTable.jobId = jobId1,
                                jobHistoryTable.timestamp = std::chrono::system_clock::now(),
                                jobHistoryTable.what = "system",
                                jobHistoryTable.state = (uint32_t) JobStatus::PENDING,
                                jobHistoryTable.details = "Job submitting"
                        )
        );

        // Create the new job object
        auto jobId2 = db->run(
                insert_into(jobTable)
                        .set(
                                jobTable.user = 1,
                                jobTable.parameters = "params1",
                                jobTable.cluster = svr.getvJwtSecrets()->at(0).clusters()[0],
                                jobTable.bundle = "whatever",
                                jobTable.application = svr.getvJwtSecrets()->at(0).name()
                        )
        );

        // Create the first state object
        db->run(
                insert_into(jobHistoryTable)
                        .set(
                                jobHistoryTable.jobId = jobId2,
                                jobHistoryTable.timestamp = std::chrono::system_clock::now(),
                                jobHistoryTable.what = "system",
                                jobHistoryTable.state = (uint32_t) JobStatus::PENDING,
                                jobHistoryTable.details = "Job submitting"
                        )
        );

        // Create the new job object
        auto jobId3 = db->run(
                insert_into(jobTable)
                        .set(
                                jobTable.user = 1,
                                jobTable.parameters = "params1",
                                jobTable.cluster = svr.getvJwtSecrets()->at(1).clusters()[0],
                                jobTable.bundle = "whatever",
                                jobTable.application = svr.getvJwtSecrets()->at(1).name()
                        )
        );

        // Create the first state object
        db->run(
                insert_into(jobHistoryTable)
                        .set(
                                jobHistoryTable.jobId = jobId3,
                                jobHistoryTable.timestamp = std::chrono::system_clock::now(),
                                jobHistoryTable.what = "system",
                                jobHistoryTable.state = (uint32_t) JobStatus::PENDING,
                                jobHistoryTable.details = "Job submitting"
                        )
        );

        // Set up the test client
        HttpClient client("localhost:8000");

        // Test unauthorized user
        auto r = client.request("PATCH", "/job/apiv1/file/", "", {{"Authorization", "not_valid"}});
        BOOST_CHECK_EQUAL(r->content.string(), "Not authorized");

        auto now = std::chrono::system_clock::now() + std::chrono::seconds{10};
        jwt::jwt_object jwtToken = {
                jwt::params::algorithm("HS256"),
                jwt::params::payload({{"userName", "User"}}),
                jwt::params::secret(svr.getvJwtSecrets()->at(0).secret())
        };
        jwtToken.add_claim("exp", now);

        // Since payload above only accepts string values, we need to set up any non-string values
        // separately
        jwtToken.payload().add_claim("userId", 5);

        // Test requesting a file list with invalid payload but authorized user
        r = client.request("PATCH", "/job/apiv1/file/", "", {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(r->content.string(), "Bad request");
        BOOST_CHECK_EQUAL(std::stoi(r->status_code), (int) SimpleWeb::StatusCode::client_error_bad_request);

        // Test requesting a file list with invalid payload but authorized user
        r = client.request("PATCH", "/job/apiv1/file/", "", {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(r->content.string(), "Bad request");
        BOOST_CHECK_EQUAL(std::stoi(r->status_code), (int) SimpleWeb::StatusCode::client_error_bad_request);

        // Test requesting a file list for job1 - should be successful because job1 was created by app1 making this request
        nlohmann::json params = {
                {"jobId",     jobId1},
                {"recursive", true},
                {"path",      "/an_awesome_path/"}
        };

        r = client.request("PATCH", "/job/apiv1/file/", params.dump(), {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(std::stoi(r->status_code), (int) SimpleWeb::StatusCode::server_error_service_unavailable);
        BOOST_CHECK_EQUAL(r->content.string(), "Remote Cluster Offline");

        // Check that app2 can request a file list for a job run by app1
        jwtToken = {
                jwt::params::algorithm("HS256"),
                jwt::params::payload({{"userName", "User"}}),
                jwt::params::secret(svr.getvJwtSecrets()->at(1).secret())
        };
        jwtToken.add_claim("exp", now);

        // Since payload above only accepts string values, we need to set up any non-string values
        // separately
        jwtToken.payload().add_claim("userId", 5);

        r = client.request("PATCH", "/job/apiv1/file/", params.dump(), {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(std::stoi(r->status_code), (int) SimpleWeb::StatusCode::server_error_service_unavailable);
        BOOST_CHECK_EQUAL(r->content.string(), "Remote Cluster Offline");

        // Check that app4 can't request a file list for a job run by app1
        jwtToken = {
                jwt::params::algorithm("HS256"),
                jwt::params::payload({{"userName", "User"}}),
                jwt::params::secret(svr.getvJwtSecrets()->at(3).secret())
        };
        jwtToken.add_claim("exp", now);

        // Since payload above only accepts string values, we need to set up any non-string values
        // separately
        jwtToken.payload().add_claim("userId", 5);

        r = client.request("PATCH", "/job/apiv1/file/", params.dump(), {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(r->content.string(), "Bad request");
        BOOST_CHECK_EQUAL(std::stoi(r->status_code), (int) SimpleWeb::StatusCode::client_error_bad_request);

        // Finished with the server
        svr.stop();

        // Clean up
        db->run(remove_from(fileDownloadTable).unconditionally());
        db->run(remove_from(jobHistoryTable).unconditionally());
        db->run(remove_from(jobTable).unconditionally());
    }

BOOST_AUTO_TEST_SUITE_END()