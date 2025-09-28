//
// Created by lewis on 22/10/20.
//

import settings;

#include <jwt/jwt.hpp>
#include <string>
#include <bits/stringfwd.h>
#include "../../tests/utils.h"
#include "../../tests/fixtures/DatabaseFixture.h"
#include "../../tests/fixtures/HttpClientFixture.h"
#include "../../tests/fixtures/HttpServerFixture.h"
#include <boost/test/unit_test.hpp>
#include <sqlpp11/sqlpp11.h>

import job_status;
import ClusterManager;
import Message;
import Cluster;
import HttpServer;

using WsServer = SimpleWeb::SocketServer<SimpleWeb::WS>;

struct JobTestDataFixture : public DatabaseFixture, public HttpServerFixture, public HttpClientFixture {
    std::shared_ptr<Cluster> cluster;
    uint64_t jobId;
    TestHttpClient client = TestHttpClient("localhost:8000");

    JobTestDataFixture() :
            cluster(std::static_pointer_cast<ClusterManager>(clusterManager)->getvClusters()->at(1))
    {
        // Create the new job object
        jobId = database->run(
                insert_into(jobTable)
                        .set(
                                jobTable.user = 1,
                                jobTable.parameters = "params1",
                                jobTable.cluster = std::static_pointer_cast<HttpServer>(httpServer)->getvJwtSecrets()->at(0).clusters()[0],
                                jobTable.bundle = "whatever",
                                jobTable.application = std::static_pointer_cast<HttpServer>(httpServer)->getvJwtSecrets()->at(0).name()
                        )
        );

        // Create the first state object
        database->run(
                insert_into(jobHistoryTable)
                        .set(
                                jobHistoryTable.jobId = jobId,
                                jobHistoryTable.timestamp = std::chrono::system_clock::now(),
                                jobHistoryTable.what = SYSTEM_SOURCE,
                                jobHistoryTable.state = static_cast<uint32_t>(JobStatus::PENDING),
                                jobHistoryTable.details = "Job submitting"
                        )
        );

        // Create the new job object
        jobId = database->run(
                insert_into(jobTable)
                        .set(
                                jobTable.user = 1,
                                jobTable.parameters = "params1",
                                jobTable.cluster = std::static_pointer_cast<HttpServer>(httpServer)->getvJwtSecrets()->at(0).clusters()[0],
                                jobTable.bundle = "whatever",
                                jobTable.application = std::static_pointer_cast<HttpServer>(httpServer)->getvJwtSecrets()->at(0).name()
                        )
        );

        // Create the first state object
        database->run(
                insert_into(jobHistoryTable)
                        .set(
                                jobHistoryTable.jobId = jobId,
                                jobHistoryTable.timestamp = std::chrono::system_clock::now(),
                                jobHistoryTable.what = SYSTEM_SOURCE,
                                jobHistoryTable.state = static_cast<uint32_t>(JobStatus::PENDING),
                                jobHistoryTable.details = "Job submitting"
                        )
        );

        // Create the new job object
        jobId = database->run(
                insert_into(jobTable)
                        .set(
                                jobTable.user = 1,
                                jobTable.parameters = "params1",
                                jobTable.cluster = std::static_pointer_cast<HttpServer>(httpServer)->getvJwtSecrets()->at(1).clusters()[0],
                                jobTable.bundle = "whatever",
                                jobTable.application = std::static_pointer_cast<HttpServer>(httpServer)->getvJwtSecrets()->at(1).name()
                        )
        );

        // Create the first state object
        database->run(
                insert_into(jobHistoryTable)
                        .set(
                                jobHistoryTable.jobId = jobId,
                                jobHistoryTable.timestamp = std::chrono::system_clock::now(),
                                jobHistoryTable.what = SYSTEM_SOURCE,
                                jobHistoryTable.state = static_cast<uint32_t>(JobStatus::PENDING),
                                jobHistoryTable.details = "Job submitting"
                        )
        );

        // Clusters should be stopped
        cluster->stop();

        auto con = std::make_shared<WsServer::Connection>(nullptr);
        cluster->setConnection(con);
    }
};

BOOST_FIXTURE_TEST_SUITE(Job_test_suite, JobTestDataFixture)
/*
 * This test suite is responsible for testing the Job HTTP Rest API
 */
    BOOST_AUTO_TEST_CASE(test_POST_new_job) {
        /*
         * Test POST requests to verify that creating new jobs works as expected
         */
        // Test unauthorized user
        auto response = client.request("POST", "/job/apiv1/job/", "", {{"Authorization", "not_valid"}});
        BOOST_CHECK_EQUAL(response->content.string(), "Not authorized");

        setJwtSecret(std::static_pointer_cast<HttpServer>(httpServer)->getvJwtSecrets()->at(0).secret());

        // Test creating a job with invalid payload but authorized user
        response = client.request("POST", "/job/apiv1/job/", "", {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(response->content.string(), "Bad request");
        BOOST_CHECK_EQUAL(std::stoi(response->status_code), (int) SimpleWeb::StatusCode::client_error_bad_request);

        response = client.request("POST", "/job/apiv1/job/", "not real json", {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(response->content.string(), "Bad request");

        // Test that an invalid cluster returns Bad request
        nlohmann::json params = {
                {"cluster", "not_real"}
        };

        response = client.request("POST", "/job/apiv1/job/", params.dump(), {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(response->content.string(), "Bad request");

        // Try to make a real job on a cluster this secret doesn't have access too
        params = {
                {"cluster",    "cluster1"},
                {"parameters", "test_params"},
                {"bundle",     "test_bundle_1"}
        };

        response = client.request("POST", "/job/apiv1/job/", params.dump(), {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(response->content.string(), "Bad request");

        // Try to make a real job on a cluster that this secret does have access to
        params = {
                {"cluster",    "cluster2"},
                {"parameters", "test_params"},
                {"bundle",     "test_bundle_1"}
        };

        // Unconnect the client
        cluster->setConnection(nullptr);

        response = client.request("POST", "/job/apiv1/job/", params.dump(), {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(std::stoi(response->status_code), (int) SimpleWeb::StatusCode::success_ok);
        BOOST_CHECK_EQUAL(response->header.find("Content-Type")->second, "application/json");

        // Check that the job that was created is correct
        nlohmann::json result;
        response->content >> result;

        uint64_t jobId = result["jobId"];
        auto jobResults =
                database->run(
                        select(all_of(jobTable))
                                .from(jobTable)
                                .where(jobTable.id == jobId)
                );

        const auto *dbJob = &jobResults.front();
        BOOST_CHECK_EQUAL(dbJob->application, std::static_pointer_cast<HttpServer>(httpServer)->getvJwtSecrets()->at(0).name());
        BOOST_CHECK_EQUAL(dbJob->cluster, std::string(params["cluster"]));
        BOOST_CHECK_EQUAL(dbJob->bundle, std::string(params["bundle"]));
        BOOST_CHECK_EQUAL(dbJob->parameters, std::string(params["parameters"]));
        BOOST_CHECK_EQUAL((uint64_t) dbJob->user, (uint64_t) 5);

        // Check that the job history that was created is correct
        auto jobHistoryResults =
                database->run(
                        select(all_of(jobHistoryTable))
                                .from(jobHistoryTable)
                                .where(jobHistoryTable.jobId == dbJob->id)
                                .order_by(jobHistoryTable.timestamp.desc())
                                .limit(1U)
                );

        const auto *dbHistory = &jobHistoryResults.front();
        BOOST_CHECK_EQUAL(dbHistory->what, SYSTEM_SOURCE);
        BOOST_CHECK_EQUAL((uint32_t) dbHistory->state, (uint32_t) JobStatus::PENDING);

        auto con = std::make_shared<WsServer::Connection>(nullptr);
        cluster->setConnection(con);

        response = client.request("POST", "/job/apiv1/job/", params.dump(), {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(std::stoi(response->status_code), (int) SimpleWeb::StatusCode::success_ok);
        BOOST_CHECK_EQUAL(response->header.find("Content-Type")->second, "application/json");

        response->content >> result;

        jobId = result["jobId"];

        auto source = std::to_string(jobId) + "_" + std::string{params["cluster"]};
        BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Medium].find(source)->second->size(), 1);

        auto ptr = *(*cluster->getqueue())[Message::Priority::Medium].find(source)->second->try_dequeue();
        auto msg = Message(*ptr);

        BOOST_CHECK_EQUAL(msg.getId(), SUBMIT_JOB);
        BOOST_CHECK_EQUAL(msg.pop_uint(), jobId);
        BOOST_CHECK_EQUAL(msg.pop_string(), std::string(params["bundle"]));
        BOOST_CHECK_EQUAL(msg.pop_string(), std::string(params["parameters"]));

        // Check that the job that was created is correct
        jobResults =
                database->run(
                        select(all_of(jobTable))
                                .from(jobTable)
                                .where(jobTable.id == jobId)
                );

        dbJob = &jobResults.front();
        BOOST_CHECK_EQUAL(dbJob->application, std::static_pointer_cast<HttpServer>(httpServer)->getvJwtSecrets()->at(0).name());
        BOOST_CHECK_EQUAL(dbJob->cluster, std::string(params["cluster"]));
        BOOST_CHECK_EQUAL(dbJob->bundle, std::string(params["bundle"]));
        BOOST_CHECK_EQUAL(dbJob->parameters, std::string(params["parameters"]));
        BOOST_CHECK_EQUAL(static_cast<uint64_t>(dbJob->user), static_cast<uint64_t>(5));

        // Check that the job history that was created is correct
        jobHistoryResults =
                database->run(
                        select(all_of(jobHistoryTable))
                                .from(jobHistoryTable)
                                .where(jobHistoryTable.jobId == dbJob->id)
                                .order_by(jobHistoryTable.timestamp.desc())
                                .limit(1U)
                );

        dbHistory = &jobHistoryResults.front();
        BOOST_CHECK_EQUAL(dbHistory->what, SYSTEM_SOURCE);
        BOOST_CHECK_EQUAL(static_cast<uint32_t>(dbHistory->state), static_cast<uint32_t>(JobStatus::SUBMITTING));
    }

    BOOST_AUTO_TEST_CASE(test_GET_get_jobs) {
        /*
         * Test GET requests to verify that fetching/filtering jobs works as expected
         */
        // Test unauthorized user
        auto response = client.request("GET", "/job/apiv1/job/", {}, {{"Authorization", "not-real"}});
        BOOST_CHECK_EQUAL(std::stoi(response->status_code), static_cast<int>(SimpleWeb::StatusCode::client_error_forbidden));

        // Test authorized app1 (Can't see jobs from app 2)
        setJwtSecret(std::static_pointer_cast<HttpServer>(httpServer)->getvJwtSecrets()->at(0).secret());

        response = client.request("GET", "/job/apiv1/job/", {}, {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(std::stoi(response->status_code), static_cast<int>(SimpleWeb::StatusCode::success_ok));
        BOOST_CHECK_EQUAL(response->header.find("Content-Type")->second, "application/json");

        nlohmann::json result;
        response->content >> result;

        // Should be exactly two results
        BOOST_CHECK_EQUAL(result.size(), 2);

        setJwtSecret(std::static_pointer_cast<HttpServer>(httpServer)->getvJwtSecrets()->at(1).secret());

        // Test authorized app2 (Can see jobs from app 1)
        response = client.request("GET", "/job/apiv1/job/", {}, {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(std::stoi(response->status_code), static_cast<int>(SimpleWeb::StatusCode::success_ok));
        BOOST_CHECK_EQUAL(response->header.find("Content-Type")->second, "application/json");

        response->content >> result;

        // Should be exactly three results
        BOOST_CHECK_EQUAL(result.size(), 3);

        setJwtSecret(std::static_pointer_cast<HttpServer>(httpServer)->getvJwtSecrets()->at(3).secret());

        // Test authorized app4 (Can't see jobs from app 1 or 2)
        response = client.request("GET", "/job/apiv1/job/", {}, {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(std::stoi(response->status_code), static_cast<int>(SimpleWeb::StatusCode::success_ok));
        BOOST_CHECK_EQUAL(response->header.find("Content-Type")->second, "application/json");

        response->content >> result;

        // Should be no results
        BOOST_CHECK_EQUAL(result.empty(), true);

        // TODO(lewis): Test job filtering
    }

    // NOLINTNEXTLINE(readability-function-cognitive-complexity)
    BOOST_AUTO_TEST_CASE(test_PATCH_cancel_job) {
        // Test unauthorized user
        auto response = client.request("PATCH", "/job/apiv1/job/", {}, {{"Authorization", "not-real"}});
        BOOST_CHECK_EQUAL(std::stoi(response->status_code), static_cast<int>(SimpleWeb::StatusCode::client_error_forbidden));

        // Test authorized user
        setJwtSecret(std::static_pointer_cast<HttpServer>(httpServer)->getvJwtSecrets()->at(0).secret());

        // Test invalid input parameters
        response = client.request("PATCH", "/job/apiv1/job/", "null", {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(std::stoi(response->status_code), (int) SimpleWeb::StatusCode::client_error_bad_request);

        // Test job which doesn't exist
        nlohmann::json params = {
                {"jobId", 0}
        };
        response = client.request("PATCH", "/job/apiv1/job/", params.dump(), {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(std::stoi(response->status_code), (int) SimpleWeb::StatusCode::client_error_bad_request);

        // Test cancelling a job with an invalid cluster
        auto jobId = database->run(
                insert_into(jobTable)
                        .set(
                                jobTable.user = 1,
                                jobTable.parameters = "params1",
                                jobTable.cluster = "not-real-cluster",
                                jobTable.bundle = "whatever",
                                jobTable.application = std::static_pointer_cast<HttpServer>(httpServer)->getvJwtSecrets()->at(0).name()
                        )
        );

        database->run(
                insert_into(jobHistoryTable)
                        .set(
                                jobHistoryTable.jobId = jobId,
                                jobHistoryTable.timestamp = std::chrono::system_clock::now(),
                                jobHistoryTable.what = SYSTEM_SOURCE,
                                jobHistoryTable.state = static_cast<uint32_t>(JobStatus::PENDING),
                                jobHistoryTable.details = "Job submitting"
                        )
        );

        params = {
                {"jobId", jobId}
        };

        response = client.request("PATCH", "/job/apiv1/job/", params.dump(), {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(std::stoi(response->status_code), static_cast<int>(SimpleWeb::StatusCode::client_error_bad_request));

        // Test cancelling a job on a cluster this application doesn't have access to
        jobId = database->run(
                insert_into(jobTable)
                        .set(
                                jobTable.user = 1,
                                jobTable.parameters = "params1",
                                jobTable.cluster = "cluster1",
                                jobTable.bundle = "whatever",
                                jobTable.application = std::static_pointer_cast<HttpServer>(httpServer)->getvJwtSecrets()->at(0).name()
                        )
        );

        database->run(
                insert_into(jobHistoryTable)
                        .set(
                                jobHistoryTable.jobId = jobId,
                                jobHistoryTable.timestamp = std::chrono::system_clock::now(),
                                jobHistoryTable.what = SYSTEM_SOURCE,
                                jobHistoryTable.state = static_cast<uint32_t>(JobStatus::PENDING),
                                jobHistoryTable.details = "Job submitting"
                        )
        );

        params = {
                {"jobId", jobId}
        };

        response = client.request("PATCH", "/job/apiv1/job/", params.dump(), {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(std::stoi(response->status_code), static_cast<int>(SimpleWeb::StatusCode::client_error_bad_request));

        // Test cancelling a pending job
        jobId = database->run(
                insert_into(jobTable)
                        .set(
                                jobTable.user = 1,
                                jobTable.parameters = "params1",
                                jobTable.cluster = std::static_pointer_cast<HttpServer>(httpServer)->getvJwtSecrets()->at(0).clusters()[0],
                                jobTable.bundle = "whatever",
                                jobTable.application = std::static_pointer_cast<HttpServer>(httpServer)->getvJwtSecrets()->at(0).name()
                        )
        );
        auto source = std::to_string(jobId) + "_" + std::static_pointer_cast<HttpServer>(httpServer)->getvJwtSecrets()->at(0).clusters()[0];

        // Create a pending job state, which should not trigger any websocket message
        database->run(
                insert_into(jobHistoryTable)
                        .set(
                                jobHistoryTable.jobId = jobId,
                                jobHistoryTable.timestamp = std::chrono::system_clock::now(),
                                jobHistoryTable.what = SYSTEM_SOURCE,
                                jobHistoryTable.state = static_cast<uint32_t>(JobStatus::PENDING),
                                jobHistoryTable.details = "Job submitting"
                        )
        );

        params = {
                {"jobId", jobId}
        };

        response = client.request("PATCH", "/job/apiv1/job/", params.dump(), {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(std::stoi(response->status_code), static_cast<int>(SimpleWeb::StatusCode::success_ok));
        BOOST_CHECK_EQUAL(response->header.find("Content-Type")->second, "application/json");
        BOOST_CHECK_MESSAGE(
                (*cluster->getqueue())[Message::Priority::Medium].find(source) == (*cluster->getqueue())[Message::Priority::Medium].end(),
                "Queue was not empty when it should have been"
        );

        nlohmann::json result;
        response->content >> result;
        BOOST_CHECK_EQUAL(result["cancelled"], jobId);

        // Verify that the job is now in cancelled state. A job which is pending (meaning it's not yet submitted to a
        // cluster) transitions directly to a cancelled state rather than via a cancelling state, since there is nothing
        // to cancel.
        auto jobHistoryResults =
                database->run(
                        select(all_of(jobHistoryTable))
                                .from(jobHistoryTable)
                                .where(jobHistoryTable.jobId == jobId)
                                .order_by(jobHistoryTable.timestamp.desc())
                                .limit(1U)
                );

        const auto *dbHistory = &jobHistoryResults.front();
        BOOST_CHECK_EQUAL(static_cast<uint32_t>(dbHistory->state), static_cast<uint32_t>(JobStatus::CANCELLED));

        // Trying to cancel the job again should result in an error since it is now in cancelling state
        response = client.request("PATCH", "/job/apiv1/job/", params.dump(), {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(std::stoi(response->status_code), static_cast<int>(SimpleWeb::StatusCode::client_error_bad_request));
        BOOST_CHECK_MESSAGE(
                (*cluster->getqueue())[Message::Priority::Medium].find(source) == (*cluster->getqueue())[Message::Priority::Medium].end(),
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
            database->run(remove_from(jobHistoryTable).unconditionally());

            // Create a job history for this state
            database->run(
                    insert_into(jobHistoryTable)
                            .set(
                                    jobHistoryTable.jobId = jobId,
                                    jobHistoryTable.timestamp = std::chrono::system_clock::now(),
                                    jobHistoryTable.what = SYSTEM_SOURCE,
                                    jobHistoryTable.state = static_cast<uint32_t>(state),
                                    jobHistoryTable.details = "Job submitting"
                            )
            );

            // Call the API to cancel the job
            response = client.request("PATCH", "/job/apiv1/job/", params.dump(), {{"Authorization", jwtToken.signature()}});
            BOOST_CHECK_EQUAL(std::stoi(response->status_code), static_cast<int>(SimpleWeb::StatusCode::success_ok));
            BOOST_CHECK_EQUAL(response->header.find("Content-Type")->second, "application/json");
            BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Medium].find(source)->second->size(), 1);

            auto ptr = *(*cluster->getqueue())[Message::Priority::Medium].find(source)->second->try_dequeue();
            auto msg = Message(*ptr);

            BOOST_CHECK_EQUAL(msg.getId(), CANCEL_JOB);
            BOOST_CHECK_EQUAL(msg.pop_uint(), jobId);

            response->content >> result;
            BOOST_CHECK_EQUAL(result["cancelled"], jobId);

            // The job status should now be CANCELLING
            jobHistoryResults =
                    database->run(
                            select(all_of(jobHistoryTable))
                                    .from(jobHistoryTable)
                                    .where(jobHistoryTable.jobId == jobId)
                                    .order_by(jobHistoryTable.timestamp.desc())
                                    .limit(1U)
                    );

            dbHistory = &jobHistoryResults.front();
            BOOST_CHECK_EQUAL((uint32_t) dbHistory->state, (uint32_t) JobStatus::CANCELLING);
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
            database->run(remove_from(jobHistoryTable).unconditionally());

            // Create a job history for this state
            database->run(
                    insert_into(jobHistoryTable)
                            .set(
                                    jobHistoryTable.jobId = jobId,
                                    jobHistoryTable.timestamp = std::chrono::system_clock::now(),
                                    jobHistoryTable.what = SYSTEM_SOURCE,
                                    jobHistoryTable.state = static_cast<uint32_t>(state),
                                    jobHistoryTable.details = "Job submitting"
                            )
            );

            // Call the API to cancel the job
            response = client.request("PATCH", "/job/apiv1/job/", params.dump(), {{"Authorization", jwtToken.signature()}});
            BOOST_CHECK_EQUAL(std::stoi(response->status_code), static_cast<int>(SimpleWeb::StatusCode::client_error_bad_request));
            BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Medium].find(source)->second->empty(), true);

            // The job status should be unchanged
            jobHistoryResults =
                    database->run(
                            select(all_of(jobHistoryTable))
                                    .from(jobHistoryTable)
                                    .where(jobHistoryTable.jobId == jobId)
                                    .order_by(jobHistoryTable.timestamp.desc())
                                    .limit(1U)
                    );

            dbHistory = &jobHistoryResults.front();
            BOOST_CHECK_EQUAL((uint32_t) dbHistory->state, (uint32_t) state);
        }
    }

    // NOLINTNEXTLINE(readability-function-cognitive-complexity)
    BOOST_AUTO_TEST_CASE(test_DELETE_delete_job) {
        // Test unauthorized user
        auto response = client.request("DELETE", "/job/apiv1/job/", {}, {{"Authorization", "not-real"}});
        BOOST_CHECK_EQUAL(std::stoi(response->status_code), static_cast<int>(SimpleWeb::StatusCode::client_error_forbidden));

        // Test authorized user
        setJwtSecret(std::static_pointer_cast<HttpServer>(httpServer)->getvJwtSecrets()->at(0).secret());

        // Test invalid input parameters
        response = client.request("DELETE", "/job/apiv1/job/", "null", {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(std::stoi(response->status_code), static_cast<int>(SimpleWeb::StatusCode::client_error_bad_request));

        // Test job which doesn't exist
        nlohmann::json params = {
                {"jobId", 0}
        };
        response = client.request("DELETE", "/job/apiv1/job/", params.dump(), {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(std::stoi(response->status_code), static_cast<int>(SimpleWeb::StatusCode::client_error_bad_request));

        // Test deleting a job with an invalid cluster
        auto jobId = database->run(
                insert_into(jobTable)
                        .set(
                                jobTable.user = 1,
                                jobTable.parameters = "params1",
                                jobTable.cluster = "not-real-cluster",
                                jobTable.bundle = "whatever",
                                jobTable.application = std::static_pointer_cast<HttpServer>(httpServer)->getvJwtSecrets()->at(0).name()
                        )
        );

        database->run(
                insert_into(jobHistoryTable)
                        .set(
                                jobHistoryTable.jobId = jobId,
                                jobHistoryTable.timestamp = std::chrono::system_clock::now(),
                                jobHistoryTable.what = SYSTEM_SOURCE,
                                jobHistoryTable.state = static_cast<uint32_t>(JobStatus::PENDING),
                                jobHistoryTable.details = "Job submitting"
                        )
        );

        params = {
                {"jobId", jobId}
        };

        response = client.request("DELETE", "/job/apiv1/job/", params.dump(), {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(std::stoi(response->status_code), static_cast<int>(SimpleWeb::StatusCode::client_error_bad_request));

        // Test deleting a job on a cluster this application doesn't have access to
        jobId = database->run(
                insert_into(jobTable)
                        .set(
                                jobTable.user = 1,
                                jobTable.parameters = "params1",
                                jobTable.cluster = "cluster1",
                                jobTable.bundle = "whatever",
                                jobTable.application = std::static_pointer_cast<HttpServer>(httpServer)->getvJwtSecrets()->at(0).name()
                        )
        );

        database->run(
                insert_into(jobHistoryTable)
                        .set(
                                jobHistoryTable.jobId = jobId,
                                jobHistoryTable.timestamp = std::chrono::system_clock::now(),
                                jobHistoryTable.what = SYSTEM_SOURCE,
                                jobHistoryTable.state = static_cast<uint32_t>(JobStatus::PENDING),
                                jobHistoryTable.details = "Job submitting"
                        )
        );

        params = {
                {"jobId", jobId}
        };

        response = client.request("DELETE", "/job/apiv1/job/", params.dump(), {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(std::stoi(response->status_code), static_cast<int>(SimpleWeb::StatusCode::client_error_bad_request));

        // Test deleting a pending job
        jobId = database->run(
                insert_into(jobTable)
                        .set(
                                jobTable.user = 1,
                                jobTable.parameters = "params1",
                                jobTable.cluster = std::static_pointer_cast<HttpServer>(httpServer)->getvJwtSecrets()->at(0).clusters()[0],
                                jobTable.bundle = "whatever",
                                jobTable.application = std::static_pointer_cast<HttpServer>(httpServer)->getvJwtSecrets()->at(0).name()
                        )
        );
        auto source = std::to_string(jobId) + "_" + std::static_pointer_cast<HttpServer>(httpServer)->getvJwtSecrets()->at(0).clusters()[0];

        // Create a pending job state, which should not trigger any websocket message
        database->run(
                insert_into(jobHistoryTable)
                        .set(
                                jobHistoryTable.jobId = jobId,
                                jobHistoryTable.timestamp = std::chrono::system_clock::now(),
                                jobHistoryTable.what = SYSTEM_SOURCE,
                                jobHistoryTable.state = static_cast<uint32_t>(JobStatus::PENDING),
                                jobHistoryTable.details = "Job submitting"
                        )
        );

        params = {
                {"jobId", jobId}
        };

        response = client.request("DELETE", "/job/apiv1/job/", params.dump(), {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(std::stoi(response->status_code), static_cast<int>(SimpleWeb::StatusCode::success_ok));
        BOOST_CHECK_EQUAL(response->header.find("Content-Type")->second, "application/json");
        BOOST_CHECK_MESSAGE(
                (*cluster->getqueue())[Message::Priority::Medium].find(source) == (*cluster->getqueue())[Message::Priority::Medium].end(),
                "Queue was not empty when it should have been"
        );

        nlohmann::json result;
        response->content >> result;
        BOOST_CHECK_EQUAL(result["deleted"], jobId);

        // Verify that the job is now in deleted state. A job which is pending (meaning it's not yet submitted to a
        // cluster) transitions directly to a deleted state rather than via a deleting state, since there is nothing
        // to delete.
        auto jobHistoryResults =
                database->run(
                        select(all_of(jobHistoryTable))
                                .from(jobHistoryTable)
                                .where(jobHistoryTable.jobId == jobId)
                                .order_by(jobHistoryTable.timestamp.desc())
                                .limit(1U)
                );

        const auto *dbHistory = &jobHistoryResults.front();
        BOOST_CHECK_EQUAL(static_cast<uint32_t>(dbHistory->state), static_cast<uint32_t>(JobStatus::DELETED));

        // Trying to delete the job again should result in an error since it is now in deleted state
        response = client.request("DELETE", "/job/apiv1/job/", params.dump(), {{"Authorization", jwtToken.signature()}});
        BOOST_CHECK_EQUAL(std::stoi(response->status_code), static_cast<int>(SimpleWeb::StatusCode::client_error_bad_request));
        BOOST_CHECK_MESSAGE(
                (*cluster->getqueue())[Message::Priority::Medium].find(source) == (*cluster->getqueue())[Message::Priority::Medium].end(),
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
            database->run(remove_from(jobHistoryTable).unconditionally());

            // Create a job history for this state
            database->run(
                    insert_into(jobHistoryTable)
                            .set(
                                    jobHistoryTable.jobId = jobId,
                                    jobHistoryTable.timestamp = std::chrono::system_clock::now(),
                                    jobHistoryTable.what = SYSTEM_SOURCE,
                                    jobHistoryTable.state = static_cast<uint32_t>(state),
                                    jobHistoryTable.details = "Job submitting"
                            )
            );

            // Call the API to delete the job
            response = client.request("DELETE", "/job/apiv1/job/", params.dump(), {{"Authorization", jwtToken.signature()}});
            BOOST_CHECK_EQUAL(std::stoi(response->status_code), static_cast<int>(SimpleWeb::StatusCode::success_ok));
            BOOST_CHECK_EQUAL(response->header.find("Content-Type")->second, "application/json");
            BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Medium].find(source)->second->size(), 1);

            auto ptr = *(*cluster->getqueue())[Message::Priority::Medium].find(source)->second->try_dequeue();
            auto msg = Message(*ptr);

            BOOST_CHECK_EQUAL(msg.getId(), DELETE_JOB);
            BOOST_CHECK_EQUAL(msg.pop_uint(), jobId);

            response->content >> result;
            BOOST_CHECK_EQUAL(result["deleted"], jobId);

            // The job status should now be DELETING
            jobHistoryResults =
                    database->run(
                            select(all_of(jobHistoryTable))
                                    .from(jobHistoryTable)
                                    .where(jobHistoryTable.jobId == jobId)
                                    .order_by(jobHistoryTable.timestamp.desc())
                                    .limit(1U)
                    );

            dbHistory = &jobHistoryResults.front();
            BOOST_CHECK_EQUAL(static_cast<uint32_t>(dbHistory->state), static_cast<uint32_t>(JobStatus::DELETING));
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
            database->run(remove_from(jobHistoryTable).unconditionally());

            // Create a job history for this state
            database->run(
                    insert_into(jobHistoryTable)
                            .set(
                                    jobHistoryTable.jobId = jobId,
                                    jobHistoryTable.timestamp = std::chrono::system_clock::now(),
                                    jobHistoryTable.what = SYSTEM_SOURCE,
                                    jobHistoryTable.state = static_cast<uint32_t>(state),
                                    jobHistoryTable.details = "Job submitting"
                            )
            );

            // Call the API to delete the job
            response = client.request("DELETE", "/job/apiv1/job/", params.dump(), {{"Authorization", jwtToken.signature()}});
            BOOST_CHECK_EQUAL(std::stoi(response->status_code), static_cast<int>(SimpleWeb::StatusCode::client_error_bad_request));
            BOOST_CHECK_EQUAL((*cluster->getqueue())[Message::Priority::Medium].find(source)->second->empty(), true);

            // The job status should be unchanged
            jobHistoryResults =
                    database->run(
                            select(all_of(jobHistoryTable))
                                    .from(jobHistoryTable)
                                    .where(jobHistoryTable.jobId == jobId)
                                    .order_by(jobHistoryTable.timestamp.desc())
                                    .limit(1U)
                    );

            dbHistory = &jobHistoryResults.front();
            BOOST_CHECK_EQUAL(static_cast<uint32_t>(dbHistory->state), static_cast<uint32_t>(state));
        }
    }
BOOST_AUTO_TEST_SUITE_END()