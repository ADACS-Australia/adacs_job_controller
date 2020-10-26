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

BOOST_AUTO_TEST_SUITE(Job_test_suite)
/*
 * This test suite is responsible for testing the Job HTTP Rest API
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

    BOOST_AUTO_TEST_CASE(test_POST_new_job) {
        /*
         * Test POST requests to verify that creating new jobs works as expected
         */

        // Delete all jobs just in case
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

        // Set up the test client
        HttpClient client("localhost:8000");

        // Test unauthorized user
        auto r = client.request("POST", "/job/apiv1/job/", "", {{"Authorization", "not_valid"}});
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

        // Test creating a job with invalid payload but authorized user
        r = client.request("POST", "/job/apiv1/job/", "", {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(r->content.string(), "Bad request");
        BOOST_CHECK_EQUAL(std::stoi(r->status_code), (int) SimpleWeb::StatusCode::client_error_bad_request);

        r = client.request("POST", "/job/apiv1/job/", "not real json", {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(r->content.string(), "Bad request");

        // Test that an invalid cluster returns Bad request
        nlohmann::json params = {
                {"cluster", "not_real"}
        };

        r = client.request("POST", "/job/apiv1/job/", params.dump(), {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(r->content.string(), "Bad request");

        // Try to make a real job on a cluster this secret doesn't have access too
        params = {
                {"cluster",    "cluster1"},
                {"parameters", "test_params"},
                {"bundle",     "test_bundle_1"}
        };

        r = client.request("POST", "/job/apiv1/job/", params.dump(), {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(r->content.string(), "Bad request");

        // Try to make a real job on a cluster that this secret does have access to
        params = {
                {"cluster",    "cluster2"},
                {"parameters", "test_params"},
                {"bundle",     "test_bundle_1"}
        };

        r = client.request("POST", "/job/apiv1/job/", params.dump(), {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(std::stoi(r->status_code), (int) SimpleWeb::StatusCode::success_ok);
        BOOST_CHECK_EQUAL(r->header.find("Content-Type")->second, "application/json");

        // Check that the job that was created is correct
        nlohmann::json result;
        r->content >> result;

        uint32_t jobId = result["jobId"];
        auto jobResults =
                db->run(
                        select(all_of(jobTable))
                                .from(jobTable)
                                .where(jobTable.id == jobId)
                );

        auto dbJob = &jobResults.front();
        BOOST_CHECK_EQUAL(dbJob->application, svr.getvJwtSecrets()->at(0).name());
        BOOST_CHECK_EQUAL(dbJob->cluster, std::string(params["cluster"]));
        BOOST_CHECK_EQUAL(dbJob->bundle, std::string(params["bundle"]));
        BOOST_CHECK_EQUAL(dbJob->parameters, std::string(params["parameters"]));
        BOOST_CHECK_EQUAL(dbJob->user, 5);

        // Check that the job history that was created is correct
        auto jobHistoryResults =
                db->run(
                        select(all_of(jobHistoryTable))
                                .from(jobHistoryTable)
                                .where(jobHistoryTable.jobId == dbJob->id)
                );

        auto dbHistory = &jobHistoryResults.front();
        BOOST_CHECK_EQUAL(dbHistory->what, "system");
        BOOST_CHECK_EQUAL(dbHistory->state, (uint32_t) JobStatus::PENDING);

        // Finished with the server
        svr.stop();

        // Clean up
        db->run(remove_from(fileDownloadTable).unconditionally());
        db->run(remove_from(jobHistoryTable).unconditionally());
        db->run(remove_from(jobTable).unconditionally());
    }

    BOOST_AUTO_TEST_CASE(test_GET_get_jobs) {
        /*
         * Test GET requests to verify that fetching/filtering jobs works as expected
         */

        // Delete all jobs just in case
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

        // Fabricate data
        // Create the new job object
        auto jobId = db->run(
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
                                jobHistoryTable.jobId = jobId,
                                jobHistoryTable.timestamp = std::chrono::system_clock::now(),
                                jobHistoryTable.what = "system",
                                jobHistoryTable.state = (uint32_t) JobStatus::PENDING,
                                jobHistoryTable.details = "Job submitting"
                        )
        );

        // Create the new job object
        jobId = db->run(
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
                                jobHistoryTable.jobId = jobId,
                                jobHistoryTable.timestamp = std::chrono::system_clock::now(),
                                jobHistoryTable.what = "system",
                                jobHistoryTable.state = (uint32_t) JobStatus::PENDING,
                                jobHistoryTable.details = "Job submitting"
                        )
        );

        // Create the new job object
        jobId = db->run(
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
                                jobHistoryTable.jobId = jobId,
                                jobHistoryTable.timestamp = std::chrono::system_clock::now(),
                                jobHistoryTable.what = "system",
                                jobHistoryTable.state = (uint32_t) JobStatus::PENDING,
                                jobHistoryTable.details = "Job submitting"
                        )
        );

        svr.start();
        std::this_thread::sleep_for(std::chrono::seconds(1));

        // Set up the test client
        HttpClient client("localhost:8000");

        // Test unauthorized user
        auto r = client.request("GET", "/job/apiv1/job/", {}, {{"Authorization", "not-real"}});
        BOOST_CHECK_EQUAL(std::stoi(r->status_code), (int) SimpleWeb::StatusCode::client_error_forbidden);

        // Test authorized app1 (Can't see jobs from app 2)
        auto now = std::chrono::system_clock::now() + std::chrono::seconds{10};
        jwt::jwt_object jwtToken = {
                jwt::params::algorithm("HS256"),
                jwt::params::payload({{"userName", "User"}}),
                jwt::params::secret(svr.getvJwtSecrets()->at(0).secret())
        };
        jwtToken.add_claim("exp", now);

        r = client.request("GET", "/job/apiv1/job/", {}, {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(std::stoi(r->status_code), (int) SimpleWeb::StatusCode::success_ok);
        BOOST_CHECK_EQUAL(r->header.find("Content-Type")->second, "application/json");

        nlohmann::json result;
        r->content >> result;

        // Should be exactly two results
        BOOST_CHECK_EQUAL(result.size(), 2);

        jwtToken = {
                jwt::params::algorithm("HS256"),
                jwt::params::payload({{"userName", "User"}}),
                jwt::params::secret(svr.getvJwtSecrets()->at(1).secret())
        };
        jwtToken.add_claim("exp", now);

        // Test authorized app2 (Can see jobs from app 1)
        r = client.request("GET", "/job/apiv1/job/", {}, {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(std::stoi(r->status_code), (int) SimpleWeb::StatusCode::success_ok);
        BOOST_CHECK_EQUAL(r->header.find("Content-Type")->second, "application/json");

        r->content >> result;

        // Should be exactly three results
        BOOST_CHECK_EQUAL(result.size(), 3);

        jwtToken = {
                jwt::params::algorithm("HS256"),
                jwt::params::payload({{"userName", "User"}}),
                jwt::params::secret(svr.getvJwtSecrets()->at(3).secret())
        };
        jwtToken.add_claim("exp", now);

        // Test authorized app4 (Can't see jobs from app 1 or 2)
        r = client.request("GET", "/job/apiv1/job/", {}, {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(std::stoi(r->status_code), (int) SimpleWeb::StatusCode::success_ok);
        BOOST_CHECK_EQUAL(r->header.find("Content-Type")->second, "application/json");

        r->content >> result;

        // Should be no results
        BOOST_CHECK_EQUAL(result.size(), 0);

        // TODO: Test job filtering

        // Finished with the server
        svr.stop();

        // Clean up
        db->run(remove_from(fileDownloadTable).unconditionally());
        db->run(remove_from(jobHistoryTable).unconditionally());
        db->run(remove_from(jobTable).unconditionally());
    }
BOOST_AUTO_TEST_SUITE_END()