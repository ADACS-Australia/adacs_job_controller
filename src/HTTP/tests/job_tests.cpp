//
// Created by lewis on 22/10/20.
//

#include <boost/test/unit_test.hpp>
#include <jwt/jwt.hpp>
#include "../../Settings.h"
#include "../../Cluster/ClusterManager.h"
#include "../HttpServer.h"
#include "../../Lib/JobStatus.h"
#include "../../tests/utils.h"


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
        TestHttpClient client("localhost:8000");

        // Test unauthorized user
        auto r = client.request("POST", "/job/apiv1/job/", "", {{"Authorization", "not_valid"}});
        BOOST_CHECK_EQUAL(r->content.string(), "Not authorized");

        auto now = std::chrono::system_clock::now() + std::chrono::minutes{10};
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
        BOOST_CHECK_EQUAL(dbHistory->what, SYSTEM_SOURCE);
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
                                jobHistoryTable.what = SYSTEM_SOURCE,
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
                                jobHistoryTable.what = SYSTEM_SOURCE,
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
                                jobHistoryTable.what = SYSTEM_SOURCE,
                                jobHistoryTable.state = (uint32_t) JobStatus::PENDING,
                                jobHistoryTable.details = "Job submitting"
                        )
        );

        svr.start();
        std::this_thread::sleep_for(std::chrono::seconds(1));

        // Set up the test client
        TestHttpClient client("localhost:8000");

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

    BOOST_AUTO_TEST_CASE(test_PATCH_cancel_job) {
        // Create a new cluster manager
        setenv(CLUSTER_CONFIG_ENV_VARIABLE, base64Encode(sClusters).c_str(), 1);
        auto manager = new ClusterManager();

        setenv(ACCESS_SECRET_ENV_VARIABLE, base64Encode(sAccess).c_str(), 1);
        auto svr = HttpServer(manager);
        svr.start();
        std::this_thread::sleep_for(std::chrono::seconds(1));

        // Bring the cluster online
        auto con = new WsServer::Connection(nullptr);
        auto cluster = manager->getCluster(svr.getvJwtSecrets()->at(0).clusters()[0]);
        cluster->setConnection(con);

        // First make sure we delete all entries from the job history table
        auto db = MySqlConnector();
        schema::JobserverJobhistory jobHistoryTable;
        schema::JobserverJob jobTable;
        db->run(remove_from(jobHistoryTable).unconditionally());
        db->run(remove_from(jobTable).unconditionally());

        // Set up the test client
        TestHttpClient client("localhost:8000");

        // Test unauthorized user
        auto r = client.request("PATCH", "/job/apiv1/job/", {}, {{"Authorization", "not-real"}});
        BOOST_CHECK_EQUAL(std::stoi(r->status_code), (int) SimpleWeb::StatusCode::client_error_forbidden);

        // Test authorized user
        auto now = std::chrono::system_clock::now() + std::chrono::seconds{10};
        jwt::jwt_object jwtToken = {
                jwt::params::algorithm("HS256"),
                jwt::params::payload({{"userName", "User"}}),
                jwt::params::secret(svr.getvJwtSecrets()->at(0).secret())
        };
        jwtToken.add_claim("exp", now);

        // Test invalid input parameters
        r = client.request("PATCH", "/job/apiv1/job/", "null", {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(std::stoi(r->status_code), (int) SimpleWeb::StatusCode::client_error_bad_request);

        // Test job which doesn't exist
        nlohmann::json params = {
                {"jobId", 0}
        };
        r = client.request("PATCH", "/job/apiv1/job/", params.dump(), {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(std::stoi(r->status_code), (int) SimpleWeb::StatusCode::client_error_bad_request);

        // Test cancelling a job with an invalid cluster
        auto jobId = db->run(
                insert_into(jobTable)
                        .set(
                                jobTable.user = 1,
                                jobTable.parameters = "params1",
                                jobTable.cluster = "not-real-cluster",
                                jobTable.bundle = "whatever",
                                jobTable.application = svr.getvJwtSecrets()->at(0).name()
                        )
        );

        db->run(
                insert_into(jobHistoryTable)
                        .set(
                                jobHistoryTable.jobId = jobId,
                                jobHistoryTable.timestamp = std::chrono::system_clock::now(),
                                jobHistoryTable.what = SYSTEM_SOURCE,
                                jobHistoryTable.state = (uint32_t) JobStatus::PENDING,
                                jobHistoryTable.details = "Job submitting"
                        )
        );

        params = {
                {"jobId", jobId}
        };

        r = client.request("PATCH", "/job/apiv1/job/", params.dump(), {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(std::stoi(r->status_code), (int) SimpleWeb::StatusCode::client_error_bad_request);

        // Test cancelling a job on a cluster this application doesn't have access to
        jobId = db->run(
                insert_into(jobTable)
                        .set(
                                jobTable.user = 1,
                                jobTable.parameters = "params1",
                                jobTable.cluster = "cluster1",
                                jobTable.bundle = "whatever",
                                jobTable.application = svr.getvJwtSecrets()->at(0).name()
                        )
        );

        db->run(
                insert_into(jobHistoryTable)
                        .set(
                                jobHistoryTable.jobId = jobId,
                                jobHistoryTable.timestamp = std::chrono::system_clock::now(),
                                jobHistoryTable.what = SYSTEM_SOURCE,
                                jobHistoryTable.state = (uint32_t) JobStatus::PENDING,
                                jobHistoryTable.details = "Job submitting"
                        )
        );

        params = {
                {"jobId", jobId}
        };

        r = client.request("PATCH", "/job/apiv1/job/", params.dump(), {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(std::stoi(r->status_code), (int) SimpleWeb::StatusCode::client_error_bad_request);

        // Test cancelling a pending job
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
        auto source = std::to_string(jobId) + "_" + svr.getvJwtSecrets()->at(0).clusters()[0];

        // Create a pending job state, which should not trigger any websocket message
        db->run(
                insert_into(jobHistoryTable)
                        .set(
                                jobHistoryTable.jobId = jobId,
                                jobHistoryTable.timestamp = std::chrono::system_clock::now(),
                                jobHistoryTable.what = SYSTEM_SOURCE,
                                jobHistoryTable.state = (uint32_t) JobStatus::PENDING,
                                jobHistoryTable.details = "Job submitting"
                        )
        );

        params = {
                {"jobId", jobId}
        };

        r = client.request("PATCH", "/job/apiv1/job/", params.dump(), {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(std::stoi(r->status_code), (int) SimpleWeb::StatusCode::success_ok);
        BOOST_CHECK_EQUAL(r->header.find("Content-Type")->second, "application/json");
        BOOST_CHECK_MESSAGE(
                (*cluster->getqueue())[Message::Priority::Medium].find(source) ==(*cluster->getqueue())[Message::Priority::Medium].end(),
                "Queue was not empty when it should have been"
        );

        nlohmann::json result;
        r->content >> result;
        BOOST_CHECK_EQUAL(result["cancelled"], jobId);

        // Verify that the job is now in cancelled state. A job which is pending (meaning it's not yet submitted to a
        // cluster) transitions directly to a cancelled state rather than via a cancelling state, since there is nothing
        // to cancel.
        auto jobHistoryResults =
                db->run(
                        select(all_of(jobHistoryTable))
                                .from(jobHistoryTable)
                                .where(jobHistoryTable.jobId == jobId)
                                .order_by(jobHistoryTable.timestamp.desc())
                                .limit(1u)
                );

        auto dbHistory = &jobHistoryResults.front();
        BOOST_CHECK_EQUAL(dbHistory->state, (uint32_t) JobStatus::CANCELLED);

        // Trying to cancel the job again should result in an error since it is now in cancelling state
        r = client.request("PATCH", "/job/apiv1/job/", params.dump(), {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(std::stoi(r->status_code), (int) SimpleWeb::StatusCode::client_error_bad_request);
        BOOST_CHECK_MESSAGE(
                (*cluster->getqueue())[Message::Priority::Medium].find(source) ==(*cluster->getqueue())[Message::Priority::Medium].end(),
                "Queue was not empty when it should have been"
        );

        // Next try valid states which are able to transition to cancelling
        auto validStates = std::vector<JobStatus>{
                JobStatus::SUBMITTING,
                JobStatus::SUBMITTED,
                JobStatus::QUEUED,
                JobStatus::RUNNING
        };

        for (auto state : validStates) {
            // Remove any existing job history
            db->run(remove_from(jobHistoryTable).unconditionally());

            // Create a job history for this state
            db->run(
                    insert_into(jobHistoryTable)
                            .set(
                                    jobHistoryTable.jobId = jobId,
                                    jobHistoryTable.timestamp = std::chrono::system_clock::now(),
                                    jobHistoryTable.what = SYSTEM_SOURCE,
                                    jobHistoryTable.state = (uint32_t) state,
                                    jobHistoryTable.details = "Job submitting"
                            )
            );

            // Call the API to cancel the job
            r = client.request("PATCH", "/job/apiv1/job/", params.dump(), {{"Authorization", jwtToken.signature()}});
            BOOST_CHECK_EQUAL(std::stoi(r->status_code), (int) SimpleWeb::StatusCode::success_ok);
            BOOST_CHECK_EQUAL(r->header.find("Content-Type")->second, "application/json");
            BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Medium].find(source)->second->size(), 1);

            auto ptr = *(*cluster->getqueue())[Message::Priority::Medium].find(source)->second->try_dequeue();
            auto msg = Message(*ptr);
            delete ptr;

            BOOST_CHECK_EQUAL(msg.getId(), CANCEL_JOB);
            BOOST_CHECK_EQUAL(msg.pop_uint(), jobId);

            r->content >> result;
            BOOST_CHECK_EQUAL(result["cancelled"], jobId);

            // The job status should now be CANCELLING
            jobHistoryResults =
                    db->run(
                            select(all_of(jobHistoryTable))
                                    .from(jobHistoryTable)
                                    .where(jobHistoryTable.jobId == jobId)
                                    .order_by(jobHistoryTable.timestamp.desc())
                                    .limit(1u)
                    );

            dbHistory = &jobHistoryResults.front();
            BOOST_CHECK_EQUAL(dbHistory->state, (uint32_t) JobStatus::CANCELLING);
        }

        // Finally test that invalid states all raise an error
        auto invalidStates = std::vector<JobStatus>{
                JobStatus::CANCELLING,
                JobStatus::CANCELLED,
                JobStatus::COMPLETED,
                JobStatus::DELETING,
                JobStatus::DELETED,
                JobStatus::ERROR,
                JobStatus::OUT_OF_MEMORY,
                JobStatus::WALL_TIME_EXCEEDED
        };

        for (auto state : invalidStates) {
            // Remove any existing job history
            db->run(remove_from(jobHistoryTable).unconditionally());

            // Create a job history for this state
            db->run(
                    insert_into(jobHistoryTable)
                            .set(
                                    jobHistoryTable.jobId = jobId,
                                    jobHistoryTable.timestamp = std::chrono::system_clock::now(),
                                    jobHistoryTable.what = SYSTEM_SOURCE,
                                    jobHistoryTable.state = (uint32_t) state,
                                    jobHistoryTable.details = "Job submitting"
                            )
            );

            // Call the API to cancel the job
            r = client.request("PATCH", "/job/apiv1/job/", params.dump(), {{"Authorization", jwtToken.signature()}});
            BOOST_CHECK_EQUAL(std::stoi(r->status_code), (int) SimpleWeb::StatusCode::client_error_bad_request);
            BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Medium].find(source)->second->size(), 0);

            // The job status should be unchanged
            jobHistoryResults =
                    db->run(
                            select(all_of(jobHistoryTable))
                                    .from(jobHistoryTable)
                                    .where(jobHistoryTable.jobId == jobId)
                                    .order_by(jobHistoryTable.timestamp.desc())
                                    .limit(1u)
                    );

            dbHistory = &jobHistoryResults.front();
            BOOST_CHECK_EQUAL(dbHistory->state, (uint32_t) state);
        }

        // Cleanup
        svr.stop();
        
        // Finally clean up all entries from the job history table
        db->run(remove_from(jobHistoryTable).unconditionally());
        db->run(remove_from(jobTable).unconditionally());

        // Cleanup
        delete manager;
        delete con;
    }

    BOOST_AUTO_TEST_CASE(test_DELETE_delete_job) {
        // Create a new cluster manager
        setenv(CLUSTER_CONFIG_ENV_VARIABLE, base64Encode(sClusters).c_str(), 1);
        auto manager = new ClusterManager();

        setenv(ACCESS_SECRET_ENV_VARIABLE, base64Encode(sAccess).c_str(), 1);
        auto svr = HttpServer(manager);
        svr.start();
        std::this_thread::sleep_for(std::chrono::seconds(1));

        // Bring the cluster online
        auto con = new WsServer::Connection(nullptr);
        auto cluster = manager->getCluster(svr.getvJwtSecrets()->at(0).clusters()[0]);
        cluster->setConnection(con);

        // First make sure we delete all entries from the job history table
        auto db = MySqlConnector();
        schema::JobserverJobhistory jobHistoryTable;
        schema::JobserverJob jobTable;
        db->run(remove_from(jobHistoryTable).unconditionally());
        db->run(remove_from(jobTable).unconditionally());

        // Set up the test client
        TestHttpClient client("localhost:8000");

        // Test unauthorized user
        auto r = client.request("DELETE", "/job/apiv1/job/", {}, {{"Authorization", "not-real"}});
        BOOST_CHECK_EQUAL(std::stoi(r->status_code), (int) SimpleWeb::StatusCode::client_error_forbidden);

        // Test authorized user
        auto now = std::chrono::system_clock::now() + std::chrono::seconds{10};
        jwt::jwt_object jwtToken = {
                jwt::params::algorithm("HS256"),
                jwt::params::payload({{"userName", "User"}}),
                jwt::params::secret(svr.getvJwtSecrets()->at(0).secret())
        };
        jwtToken.add_claim("exp", now);

        // Test invalid input parameters
        r = client.request("DELETE", "/job/apiv1/job/", "null", {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(std::stoi(r->status_code), (int) SimpleWeb::StatusCode::client_error_bad_request);

        // Test job which doesn't exist
        nlohmann::json params = {
                {"jobId", 0}
        };
        r = client.request("DELETE", "/job/apiv1/job/", params.dump(), {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(std::stoi(r->status_code), (int) SimpleWeb::StatusCode::client_error_bad_request);

        // Test deleting a job with an invalid cluster
        auto jobId = db->run(
                insert_into(jobTable)
                        .set(
                                jobTable.user = 1,
                                jobTable.parameters = "params1",
                                jobTable.cluster = "not-real-cluster",
                                jobTable.bundle = "whatever",
                                jobTable.application = svr.getvJwtSecrets()->at(0).name()
                        )
        );

        db->run(
                insert_into(jobHistoryTable)
                        .set(
                                jobHistoryTable.jobId = jobId,
                                jobHistoryTable.timestamp = std::chrono::system_clock::now(),
                                jobHistoryTable.what = SYSTEM_SOURCE,
                                jobHistoryTable.state = (uint32_t) JobStatus::PENDING,
                                jobHistoryTable.details = "Job submitting"
                        )
        );

        params = {
                {"jobId", jobId}
        };

        r = client.request("DELETE", "/job/apiv1/job/", params.dump(), {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(std::stoi(r->status_code), (int) SimpleWeb::StatusCode::client_error_bad_request);

        // Test deleting a job on a cluster this application doesn't have access to
        jobId = db->run(
                insert_into(jobTable)
                        .set(
                                jobTable.user = 1,
                                jobTable.parameters = "params1",
                                jobTable.cluster = "cluster1",
                                jobTable.bundle = "whatever",
                                jobTable.application = svr.getvJwtSecrets()->at(0).name()
                        )
        );

        db->run(
                insert_into(jobHistoryTable)
                        .set(
                                jobHistoryTable.jobId = jobId,
                                jobHistoryTable.timestamp = std::chrono::system_clock::now(),
                                jobHistoryTable.what = SYSTEM_SOURCE,
                                jobHistoryTable.state = (uint32_t) JobStatus::PENDING,
                                jobHistoryTable.details = "Job submitting"
                        )
        );

        params = {
                {"jobId", jobId}
        };

        r = client.request("DELETE", "/job/apiv1/job/", params.dump(), {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(std::stoi(r->status_code), (int) SimpleWeb::StatusCode::client_error_bad_request);

        // Test deleting a pending job
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
        auto source = std::to_string(jobId) + "_" + svr.getvJwtSecrets()->at(0).clusters()[0];

        // Create a pending job state, which should not trigger any websocket message
        db->run(
                insert_into(jobHistoryTable)
                        .set(
                                jobHistoryTable.jobId = jobId,
                                jobHistoryTable.timestamp = std::chrono::system_clock::now(),
                                jobHistoryTable.what = SYSTEM_SOURCE,
                                jobHistoryTable.state = (uint32_t) JobStatus::PENDING,
                                jobHistoryTable.details = "Job submitting"
                        )
        );

        params = {
                {"jobId", jobId}
        };

        r = client.request("DELETE", "/job/apiv1/job/", params.dump(), {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(std::stoi(r->status_code), (int) SimpleWeb::StatusCode::success_ok);
        BOOST_CHECK_EQUAL(r->header.find("Content-Type")->second, "application/json");
        BOOST_CHECK_MESSAGE(
                (*cluster->getqueue())[Message::Priority::Medium].find(source) ==(*cluster->getqueue())[Message::Priority::Medium].end(),
                "Queue was not empty when it should have been"
        );

        nlohmann::json result;
        r->content >> result;
        BOOST_CHECK_EQUAL(result["deleted"], jobId);

        // Verify that the job is now in deleted state. A job which is pending (meaning it's not yet submitted to a
        // cluster) transitions directly to a deleted state rather than via a deleting state, since there is nothing
        // to delete.
        auto jobHistoryResults =
                db->run(
                        select(all_of(jobHistoryTable))
                                .from(jobHistoryTable)
                                .where(jobHistoryTable.jobId == jobId)
                                .order_by(jobHistoryTable.timestamp.desc())
                                .limit(1u)
                );

        auto dbHistory = &jobHistoryResults.front();
        BOOST_CHECK_EQUAL(dbHistory->state, (uint32_t) JobStatus::DELETED);

        // Trying to delete the job again should result in an error since it is now in deleted state
        r = client.request("DELETE", "/job/apiv1/job/", params.dump(), {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(std::stoi(r->status_code), (int) SimpleWeb::StatusCode::client_error_bad_request);
        BOOST_CHECK_MESSAGE(
                (*cluster->getqueue())[Message::Priority::Medium].find(source) ==(*cluster->getqueue())[Message::Priority::Medium].end(),
                "Queue was not empty when it should have been"
        );

        // Next try valid states which are able to transition to deleting
        auto validStates = std::vector<JobStatus>{
                JobStatus::CANCELLED,
                JobStatus::ERROR,
                JobStatus::WALL_TIME_EXCEEDED,
                JobStatus::OUT_OF_MEMORY,
                JobStatus::COMPLETED
        };

        for (auto state : validStates) {
            // Remove any existing job history
            db->run(remove_from(jobHistoryTable).unconditionally());

            // Create a job history for this state
            db->run(
                    insert_into(jobHistoryTable)
                            .set(
                                    jobHistoryTable.jobId = jobId,
                                    jobHistoryTable.timestamp = std::chrono::system_clock::now(),
                                    jobHistoryTable.what = SYSTEM_SOURCE,
                                    jobHistoryTable.state = (uint32_t) state,
                                    jobHistoryTable.details = "Job submitting"
                            )
            );

            // Call the API to delete the job
            r = client.request("DELETE", "/job/apiv1/job/", params.dump(), {{"Authorization", jwtToken.signature()}});
            BOOST_CHECK_EQUAL(std::stoi(r->status_code), (int) SimpleWeb::StatusCode::success_ok);
            BOOST_CHECK_EQUAL(r->header.find("Content-Type")->second, "application/json");
            BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Medium].find(source)->second->size(), 1);

            auto ptr = *(*cluster->getqueue())[Message::Priority::Medium].find(source)->second->try_dequeue();
            auto msg = Message(*ptr);
            delete ptr;

            BOOST_CHECK_EQUAL(msg.getId(), DELETE_JOB);
            BOOST_CHECK_EQUAL(msg.pop_uint(), jobId);

            r->content >> result;
            BOOST_CHECK_EQUAL(result["deleted"], jobId);

            // The job status should now be DELETING
            jobHistoryResults =
                    db->run(
                            select(all_of(jobHistoryTable))
                                    .from(jobHistoryTable)
                                    .where(jobHistoryTable.jobId == jobId)
                                    .order_by(jobHistoryTable.timestamp.desc())
                                    .limit(1u)
                    );

            dbHistory = &jobHistoryResults.front();
            BOOST_CHECK_EQUAL(dbHistory->state, (uint32_t) JobStatus::DELETING);
        }

        // Finally test that invalid states all raise an error
        auto invalidStates = std::vector<JobStatus>{
                JobStatus::SUBMITTING,
                JobStatus::SUBMITTED,
                JobStatus::QUEUED,
                JobStatus::RUNNING,
                JobStatus::CANCELLING,
                JobStatus::DELETING,
                JobStatus::DELETED
        };

        for (auto state : invalidStates) {
            // Remove any existing job history
            db->run(remove_from(jobHistoryTable).unconditionally());

            // Create a job history for this state
            db->run(
                    insert_into(jobHistoryTable)
                            .set(
                                    jobHistoryTable.jobId = jobId,
                                    jobHistoryTable.timestamp = std::chrono::system_clock::now(),
                                    jobHistoryTable.what = SYSTEM_SOURCE,
                                    jobHistoryTable.state = (uint32_t) state,
                                    jobHistoryTable.details = "Job submitting"
                            )
            );

            // Call the API to delete the job
            r = client.request("DELETE", "/job/apiv1/job/", params.dump(), {{"Authorization", jwtToken.signature()}});
            BOOST_CHECK_EQUAL(std::stoi(r->status_code), (int) SimpleWeb::StatusCode::client_error_bad_request);
            BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Medium].find(source)->second->size(), 0);

            // The job status should be unchanged
            jobHistoryResults =
                    db->run(
                            select(all_of(jobHistoryTable))
                                    .from(jobHistoryTable)
                                    .where(jobHistoryTable.jobId == jobId)
                                    .order_by(jobHistoryTable.timestamp.desc())
                                    .limit(1u)
                    );

            dbHistory = &jobHistoryResults.front();
            BOOST_CHECK_EQUAL(dbHistory->state, (uint32_t) state);
        }

        // Cleanup
        svr.stop();
        
        // Finally clean up all entries from the job history table
        db->run(remove_from(jobHistoryTable).unconditionally());
        db->run(remove_from(jobTable).unconditionally());

        // Cleanup
        delete manager;
        delete con;
    }
BOOST_AUTO_TEST_SUITE_END()