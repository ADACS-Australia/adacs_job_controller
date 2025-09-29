//
// Created by lewis on 10/8/22.
//


import settings;
#include <random>
#include <utility>

#include <boost/lexical_cast.hpp>
#include <boost/test/unit_test.hpp>
#include <boost/uuid/uuid_io.hpp>
#include <jwt/jwt.hpp>

#include "../../tests/fixtures/DatabaseFixture.h"
#include "../../tests/fixtures/WebSocketClientFixture.h"

import job_status;
import sClusterJob;
import sClusterJobStatus;
import sBundleJob;
import Message;
import Cluster;
import ClusterManager;

// NOLINTBEGIN(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)

struct ClusterDBTestDataFixture : public DatabaseFixture, public WebSocketClientFixture
{
    std::vector<std::shared_ptr<Message>> receivedMessages;
    bool bReady = false;
    std::shared_ptr<Cluster> onlineCluster;
    std::promise<void> promMessageReceived;

    ClusterDBTestDataFixture()
        : onlineCluster(std::static_pointer_cast<ClusterManager>(clusterManager)->getvClusters()->front())
    {
        websocketClient->on_message = [&]([[maybe_unused]] auto connection, auto in_message) {
            onWebsocketMessage(in_message);
        };

        startWebSocketClient();

        // Wait for the client to connect
        while (!bReady)
        {
            // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
            std::this_thread::sleep_for(std::chrono::milliseconds(10));
        }
    }

    void onWebsocketMessage(auto in_message)
    {
        auto data = in_message->string();
        auto msg  = std::make_shared<Message>(std::vector<uint8_t>(data.begin(), data.end()));

        // Don't parse the message if the ws connection is ready
        if (!bReady)
        {
            if (msg->getId() == SERVER_READY)
            {
                bReady = true;
                return;
            }
        }

        receivedMessages.emplace_back(msg);
        try
        {
            // This may raise in some circumstances while shutting down the websocket
            promMessageReceived.set_value();
        }
        catch (std::future_error&)
        {}
    }

    auto runCluster() -> std::shared_ptr<Message>
    {
        promMessageReceived.get_future().wait();
        promMessageReceived = std::promise<void>();
        auto result         = receivedMessages[0];
        receivedMessages.clear();

        return result;
    }
};

BOOST_FIXTURE_TEST_SUITE(Cluster_DB_test_suite, ClusterDBTestDataFixture)

BOOST_AUTO_TEST_CASE(test_db_job_get_by_job_id_success_job_exists)
{
    // Create a Cluster Job record
    sClusterJob job{.jobId = 4321, .bundleHash = "test_hash", .workingDirectory = "/test/working/directory/"};
    job.save(onlineCluster->getName());

    // Try to fetch it
    Message msg(DB_JOB_GET_BY_JOB_ID);
    msg.push_ulong(1234);  // db request id
    msg.push_ulong(job.jobId);
    onlineCluster->handleMessage(msg);

    // Get the result and verify success
    auto result = runCluster();

    BOOST_CHECK_EQUAL(result->pop_ulong(), 1234);  // db request id
    BOOST_CHECK_EQUAL(result->pop_bool(), true);   // success?
    BOOST_CHECK_EQUAL(result->pop_uint(), 1);

    auto resultJob = sClusterJob::fromMessage(*result);
    BOOST_CHECK_EQUAL(resultJob.equals(job), true);
}

BOOST_AUTO_TEST_CASE(test_db_job_get_by_job_id_success_job_not_exist)
{
    // Create a Cluster Job record, but don't save it
    sClusterJob job;

    // Try to fetch it
    Message msg(DB_JOB_GET_BY_JOB_ID);
    msg.push_ulong(1234);  // db request id
    msg.push_ulong(4321);
    onlineCluster->handleMessage(msg);

    // Get the result and verify success
    auto result = runCluster();

    BOOST_CHECK_EQUAL(result->pop_ulong(), 1234);  // db request id
    BOOST_CHECK_EQUAL(result->pop_bool(), true);   // success?
    BOOST_CHECK_EQUAL(result->pop_uint(), 1);

    // Job should be empty/equal to default
    auto resultJob = sClusterJob::fromMessage(*result);
    BOOST_CHECK_EQUAL(resultJob.equals(job), true);
}

BOOST_AUTO_TEST_CASE(test_db_job_get_by_id_success_job_exists)
{
    // Create a Cluster Job record
    sClusterJob job{.jobId = 4321, .bundleHash = "test_hash", .workingDirectory = "/test/working/directory/"};
    job.save(onlineCluster->getName());

    // Try to fetch it
    Message msg(DB_JOB_GET_BY_ID);
    msg.push_ulong(1234);  // db request id
    msg.push_ulong(job.id);
    onlineCluster->handleMessage(msg);

    // Get the result and verify success
    auto result = runCluster();

    BOOST_CHECK_EQUAL(result->pop_ulong(), 1234);  // db request id
    BOOST_CHECK_EQUAL(result->pop_bool(), true);   // success?
    BOOST_CHECK_EQUAL(result->pop_uint(), 1);

    auto resultJob = sClusterJob::fromMessage(*result);
    BOOST_CHECK_EQUAL(resultJob.equals(job), true);
}

BOOST_AUTO_TEST_CASE(test_db_job_get_by_id_success_job_not_exist)
{
    // Create a Cluster Job record, but don't save it
    sClusterJob job;

    // Try to fetch it
    Message msg(DB_JOB_GET_BY_ID);
    msg.push_ulong(1234);  // db request id
    msg.push_ulong(4321);
    onlineCluster->handleMessage(msg);

    // Get the result and verify success
    auto result = runCluster();

    BOOST_CHECK_EQUAL(result->pop_ulong(), 1234);  // db request id
    BOOST_CHECK_EQUAL(result->pop_bool(), true);   // success?
    BOOST_CHECK_EQUAL(result->pop_uint(), 1);

    // Job should be empty/equal to default
    auto resultJob = sClusterJob::fromMessage(*result);
    BOOST_CHECK_EQUAL(resultJob.equals(job), true);
}

BOOST_AUTO_TEST_CASE(test_db_job_get_running_cluster_jobs_none)
{
    // Create a Cluster Job record, but don't save it
    sClusterJob job;

    // Try to fetch it
    Message msg(DB_JOB_GET_RUNNING_JOBS);
    msg.push_ulong(1234);  // db request id
    onlineCluster->handleMessage(msg);

    // Get the result and verify success
    auto result = runCluster();

    BOOST_CHECK_EQUAL(result->pop_ulong(), 1234);  // db request id
    BOOST_CHECK_EQUAL(result->pop_bool(), true);   // success?
    BOOST_CHECK_EQUAL(result->pop_uint(), 0);
}

BOOST_AUTO_TEST_CASE(test_db_job_get_running_cluster_jobs)
{
    // Create several Cluster Jobs
    sClusterJob job1{.jobId = 1234, .submitting = false, .running = true};
    job1.save(onlineCluster->getName());

    sClusterJob job2{.jobId = 1234, .submitting = false, .running = true};
    job2.save(onlineCluster->getName());

    // running is false, shouldn't show up
    sClusterJob job3{.jobId = 1234, .submitting = false, .running = false};
    job3.save(onlineCluster->getName());

    // different cluster, shouldn't show up
    sClusterJob job4{.jobId = 1234, .submitting = false, .running = true};
    job4.save(onlineCluster->getName() + "not_this_cluster");

    // Try to fetch it
    Message msg(DB_JOB_GET_RUNNING_JOBS);
    msg.push_ulong(1234);  // db request id
    onlineCluster->handleMessage(msg);

    // Get the result and verify success
    auto result = runCluster();

    BOOST_CHECK_EQUAL(result->pop_ulong(), 1234);  // db request id
    BOOST_CHECK_EQUAL(result->pop_bool(), true);   // success?
    BOOST_CHECK_EQUAL(result->pop_uint(), 2);

    // Job 1 and 2 should be added
    auto resultJob = sClusterJob::fromMessage(*result);
    BOOST_CHECK_EQUAL(resultJob.equals(job1), true);
    resultJob = sClusterJob::fromMessage(*result);
    BOOST_CHECK_EQUAL(resultJob.equals(job2), true);
}

BOOST_AUTO_TEST_CASE(test_db_job_delete_job)
{
    // Create several Cluster Jobs
    sClusterJob job1{.jobId = 1234, .submitting = false, .running = true};
    job1.save(onlineCluster->getName());

    sClusterJob job2{.jobId = 1234, .submitting = false, .running = true};
    job2.save(onlineCluster->getName());

    // Try to fetch it
    Message msg(DB_JOB_DELETE);
    msg.push_ulong(1234);  // db request id
    msg.push_ulong(job1.id);
    onlineCluster->handleMessage(msg);

    // Get the result and verify success
    auto result = runCluster();

    BOOST_CHECK_EQUAL(result->pop_ulong(), 1234);  // db request id
    BOOST_CHECK_EQUAL(result->pop_bool(), true);   // success?

    // Job 1 should not exist
    auto resultJob = sClusterJob::getById(job1.id, onlineCluster->getName());
    sClusterJob emptyJob;
    BOOST_CHECK_EQUAL(resultJob.equals(emptyJob), true);

    // Job 2 should remain
    resultJob = sClusterJob::getById(job2.id, onlineCluster->getName());
    BOOST_CHECK_EQUAL(resultJob.equals(job2), true);
}

BOOST_AUTO_TEST_CASE(test_db_job_save_job)
{
    // Create a Cluster Job record
    sClusterJob job{.jobId            = 4321,
                    .bundleHash       = "test_hash",
                    .workingDirectory = "/test/working/directory/",
                    .deleting         = true,
                    .deleted          = true};

    // Try to fetch it
    Message msg(DB_JOB_SAVE);
    msg.push_ulong(1234);  // db request id
    job.toMessage(msg);
    onlineCluster->handleMessage(msg);

    // Get the result and verify success
    auto result = runCluster();

    BOOST_CHECK_EQUAL(result->pop_ulong(), 1234);  // db request id
    BOOST_CHECK_EQUAL(result->pop_bool(), true);   // success?

    // Job ID is returned in the message
    job.id = result->pop_ulong();

    auto resultJob = sClusterJob::getById(job.id, onlineCluster->getName());
    BOOST_CHECK_EQUAL(resultJob.equals(job), true);

    // Save again, job should be updated but ID should not change
    job.workingDirectory = "/a/different/working/directory";
    msg                  = Message(DB_JOB_SAVE);
    msg.push_ulong(12345);  // db request id
    job.toMessage(msg);
    onlineCluster->handleMessage(msg);

    // Get the result and verify success
    result = runCluster();

    BOOST_CHECK_EQUAL(result->pop_ulong(), 12345);  // db request id
    BOOST_CHECK_EQUAL(result->pop_bool(), true);    // success?

    BOOST_CHECK_EQUAL(job.id, result->pop_ulong());

    resultJob = sClusterJob::getById(job.id, onlineCluster->getName());
    BOOST_CHECK_EQUAL(resultJob.equals(job), true);
}

BOOST_AUTO_TEST_CASE(test_db_job_status_get_by_job_id_and_what_success)
{
    // Create a Cluster Job record
    sClusterJob job{.jobId = 4321, .bundleHash = "test_hash", .workingDirectory = "/test/working/directory/"};
    job.save(onlineCluster->getName());

    sClusterJob job2{.jobId = 43210, .bundleHash = "test_hash", .workingDirectory = "/test/working/directory/"};
    job2.save(onlineCluster->getName());

    sClusterJobStatus status1{.jobId = job.id, .what = "test_what", .state = JobStatus::COMPLETED};
    status1.save(onlineCluster->getName());

    sClusterJobStatus status2{.jobId = job.id, .what = "test_what", .state = JobStatus::COMPLETED};
    status2.save(onlineCluster->getName());

    // Different job id should be excluded
    sClusterJobStatus status3{.jobId = job2.id, .what = "test_what", .state = JobStatus::COMPLETED};
    status3.save(onlineCluster->getName());

    // Different what should be excluded
    sClusterJobStatus status4{.jobId = job.id, .what = "test_what_different", .state = JobStatus::COMPLETED};
    status4.save(onlineCluster->getName());

    // Try to fetch it
    Message msg(DB_JOBSTATUS_GET_BY_JOB_ID_AND_WHAT);
    msg.push_ulong(1234);  // db request id
    msg.push_ulong(status1.jobId);
    msg.push_string(status1.what);
    onlineCluster->handleMessage(msg);

    // Get the result and verify success
    auto result = runCluster();

    BOOST_CHECK_EQUAL(result->pop_ulong(), 1234);  // db request id
    BOOST_CHECK_EQUAL(result->pop_bool(), true);   // success?
    BOOST_CHECK_EQUAL(result->pop_uint(), 2);

    auto resultStatus = sClusterJobStatus::fromMessage(*result);
    BOOST_CHECK_EQUAL(resultStatus.equals(status1), true);

    resultStatus = sClusterJobStatus::fromMessage(*result);
    BOOST_CHECK_EQUAL(resultStatus.equals(status2), true);
}

BOOST_AUTO_TEST_CASE(test_db_job_status_get_by_job_id_and_what_invalid_job)
{
    // Create a Cluster Job record
    sClusterJob job{.jobId = 4321, .bundleHash = "test_hash", .workingDirectory = "/test/working/directory/"};
    job.save(onlineCluster->getName());

    // Try to fetch it
    Message msg(DB_JOBSTATUS_GET_BY_JOB_ID_AND_WHAT);
    msg.push_ulong(1234);  // db request id
    msg.push_ulong(job.id + 10);
    msg.push_string("doesn't matter");
    onlineCluster->handleMessage(msg);

    // Get the result and verify success
    auto result = runCluster();

    BOOST_CHECK_EQUAL(result->pop_ulong(), 1234);  // db request id
    BOOST_CHECK_EQUAL(result->pop_bool(), false);  // success?
}

BOOST_AUTO_TEST_CASE(test_db_job_status_get_by_job_id_success)
{
    // Create a Cluster Job record
    sClusterJob job{.jobId = 4321, .bundleHash = "test_hash", .workingDirectory = "/test/working/directory/"};
    job.save(onlineCluster->getName());

    sClusterJob job2{.jobId = 43210, .bundleHash = "test_hash", .workingDirectory = "/test/working/directory/"};
    job2.save(onlineCluster->getName());

    sClusterJobStatus status1{.jobId = job.id, .what = "test_what", .state = JobStatus::COMPLETED};
    status1.save(onlineCluster->getName());

    sClusterJobStatus status2{.jobId = job.id, .what = "test_what", .state = JobStatus::COMPLETED};
    status2.save(onlineCluster->getName());

    // Different job id should be excluded
    sClusterJobStatus status3{.jobId = job2.id, .what = "test_what", .state = JobStatus::COMPLETED};
    status3.save(onlineCluster->getName());

    // Different what should be excluded
    sClusterJobStatus status4{.jobId = job.id, .what = "test_what_different", .state = JobStatus::COMPLETED};
    status4.save(onlineCluster->getName());

    // Try to fetch it
    Message msg(DB_JOBSTATUS_GET_BY_JOB_ID);
    msg.push_ulong(1234);  // db request id
    msg.push_ulong(status1.jobId);
    onlineCluster->handleMessage(msg);

    // Get the result and verify success
    auto result = runCluster();

    BOOST_CHECK_EQUAL(result->pop_ulong(), 1234);  // db request id
    BOOST_CHECK_EQUAL(result->pop_bool(), true);   // success?
    BOOST_CHECK_EQUAL(result->pop_uint(), 3);

    auto resultStatus = sClusterJobStatus::fromMessage(*result);
    BOOST_CHECK_EQUAL(resultStatus.equals(status1), true);

    resultStatus = sClusterJobStatus::fromMessage(*result);
    BOOST_CHECK_EQUAL(resultStatus.equals(status2), true);

    resultStatus = sClusterJobStatus::fromMessage(*result);
    BOOST_CHECK_EQUAL(resultStatus.equals(status4), true);
}

BOOST_AUTO_TEST_CASE(test_db_job_status_get_by_job_id_invalid_job)
{
    // Create a Cluster Job record
    sClusterJob job{.jobId = 4321, .bundleHash = "test_hash", .workingDirectory = "/test/working/directory/"};
    job.save(onlineCluster->getName());

    // Try to fetch it
    Message msg(DB_JOBSTATUS_GET_BY_JOB_ID);
    msg.push_ulong(1234);  // db request id
    msg.push_ulong(job.id + 10);
    onlineCluster->handleMessage(msg);

    // Get the result and verify success
    auto result = runCluster();

    BOOST_CHECK_EQUAL(result->pop_ulong(), 1234);  // db request id
    BOOST_CHECK_EQUAL(result->pop_bool(), false);  // success?
}

BOOST_AUTO_TEST_CASE(test_db_job_status_delete_by_id_list)
{
    // Create a Cluster Job record
    sClusterJob job{.jobId = 4321, .bundleHash = "test_hash", .workingDirectory = "/test/working/directory/"};
    job.save(onlineCluster->getName());

    sClusterJob job2{.jobId = 43210, .bundleHash = "test_hash", .workingDirectory = "/test/working/directory/"};
    job2.save(onlineCluster->getName());

    sClusterJobStatus status1{.jobId = job.id, .what = "test_what", .state = JobStatus::COMPLETED};
    status1.save(onlineCluster->getName());

    sClusterJobStatus status2{.jobId = job.id, .what = "test_what", .state = JobStatus::COMPLETED};
    status2.save(onlineCluster->getName());

    // Different job id should be excluded
    sClusterJobStatus status3{.jobId = job2.id, .what = "test_what", .state = JobStatus::COMPLETED};
    status3.save(onlineCluster->getName());

    // Different what should be excluded
    sClusterJobStatus status4{.jobId = job.id, .what = "test_what_different", .state = JobStatus::COMPLETED};
    status4.save(onlineCluster->getName());

    // Try to fetch it
    Message msg(DB_JOBSTATUS_DELETE_BY_ID_LIST);
    msg.push_ulong(1234);  // db request id
    msg.push_uint(2);
    msg.push_ulong(status1.id);
    msg.push_ulong(status2.id);
    onlineCluster->handleMessage(msg);

    // Get the result and verify success
    auto result = runCluster();

    BOOST_CHECK_EQUAL(result->pop_ulong(), 1234);  // db request id
    BOOST_CHECK_EQUAL(result->pop_bool(), true);   // success?

    // We deleted status1 and status2, status4 should be the only remaining status record for job
    auto resultStatus = sClusterJobStatus::getJobStatusByJobId(job.id, onlineCluster->getName());
    BOOST_CHECK_EQUAL(resultStatus.size(), 1);
    BOOST_CHECK_EQUAL(resultStatus[0].equals(status4), true);
}

BOOST_AUTO_TEST_CASE(test_db_job_status_save_success)
{
    // Create a Cluster Job record
    sClusterJob job{.jobId = 4321, .bundleHash = "test_hash", .workingDirectory = "/test/working/directory/"};
    job.save(onlineCluster->getName());

    sClusterJobStatus status{.jobId = job.id, .what = "test_what", .state = JobStatus::COMPLETED};

    // Try to save it
    Message msg(DB_JOBSTATUS_SAVE);
    msg.push_ulong(1234);  // db request id
    status.toMessage(msg);
    onlineCluster->handleMessage(msg);

    // Get the result and verify success
    auto result = runCluster();

    BOOST_CHECK_EQUAL(result->pop_ulong(), 1234);  // db request id
    BOOST_CHECK_EQUAL(result->pop_bool(), true);   // success?

    // Job ID is returned in the message
    status.id = result->pop_ulong();

    auto resultStatus = sClusterJobStatus::getJobStatusByJobId(job.id, onlineCluster->getName());
    BOOST_CHECK_EQUAL(resultStatus[0].equals(status), true);

    // Save again, status should be updated but ID should not change
    status.state = JobStatus::QUEUED;
    msg          = Message(DB_JOBSTATUS_SAVE);
    msg.push_ulong(12345);  // db request id
    status.toMessage(msg);
    onlineCluster->handleMessage(msg);

    // Get the result and verify success
    result = runCluster();

    BOOST_CHECK_EQUAL(result->pop_ulong(), 12345);  // db request id
    BOOST_CHECK_EQUAL(result->pop_bool(), true);    // success?

    BOOST_CHECK_EQUAL(status.id, result->pop_ulong());

    resultStatus = sClusterJobStatus::getJobStatusByJobId(job.id, onlineCluster->getName());
    BOOST_CHECK_EQUAL(resultStatus[0].equals(status), true);
}

BOOST_AUTO_TEST_CASE(test_db_job_status_save_invalid_job)
{
    // Create a Cluster Job record
    sClusterJob job{.jobId = 4321, .bundleHash = "test_hash", .workingDirectory = "/test/working/directory/"};
    job.save(onlineCluster->getName());

    sClusterJobStatus status{.jobId = job.id + 10, .what = "test_what", .state = JobStatus::COMPLETED};

    // Try to save it
    Message msg(DB_JOBSTATUS_SAVE);
    msg.push_ulong(1234);  // db request id
    status.toMessage(msg);
    onlineCluster->handleMessage(msg);

    // Get the result and verify success
    auto result = runCluster();

    BOOST_CHECK_EQUAL(result->pop_ulong(), 1234);  // db request id
    BOOST_CHECK_EQUAL(result->pop_bool(), false);  // success?
}

BOOST_AUTO_TEST_CASE(test_db_bundle_job_save_success)
{
    auto bundleHash = generateUUID();

    // Create a Bundle Job record
    sBundleJob job{.id = 0, .content = "test content"};

    // Try to save it
    Message msg(DB_BUNDLE_CREATE_OR_UPDATE_JOB);
    msg.push_ulong(1234);  // db request id
    msg.push_string(bundleHash);
    job.toMessage(msg);
    onlineCluster->handleMessage(msg);

    // Get the result and verify success
    auto result = runCluster();

    BOOST_CHECK_EQUAL(result->pop_ulong(), 1234);  // db request id
    BOOST_CHECK_EQUAL(result->pop_bool(), true);   // success?

    // Job ID is returned in the message
    job.id = result->pop_ulong();

    auto resultJob = sBundleJob::getById(job.id, onlineCluster->getName(), bundleHash);
    BOOST_CHECK_EQUAL(resultJob.equals(job), true);

    // Save again, status should be updated but ID should not change
    job.content = "new test content";
    msg         = Message(DB_BUNDLE_CREATE_OR_UPDATE_JOB);
    msg.push_ulong(12345);  // db request id
    msg.push_string(bundleHash);
    job.toMessage(msg);
    onlineCluster->handleMessage(msg);

    // Get the result and verify success
    result = runCluster();

    BOOST_CHECK_EQUAL(result->pop_ulong(), 12345);  // db request id
    BOOST_CHECK_EQUAL(result->pop_bool(), true);    // success?

    BOOST_CHECK_EQUAL(job.id, result->pop_ulong());

    resultJob = sBundleJob::getById(job.id, onlineCluster->getName(), bundleHash);
    BOOST_CHECK_EQUAL(resultJob.equals(job), true);
}

BOOST_AUTO_TEST_CASE(test_db_bundle_job_save_error)
{
    auto bundleHash = generateUUID();

    // Create a Bundle Job record
    sBundleJob job{.id = 0, .content = "test content"};

    // Try to save it
    Message msg(DB_BUNDLE_CREATE_OR_UPDATE_JOB);
    msg.push_ulong(1234);  // db request id
    msg.push_string(bundleHash);
    job.toMessage(msg);
    onlineCluster->handleMessage(msg);

    // Get the result and verify success
    auto result = runCluster();

    BOOST_CHECK_EQUAL(result->pop_ulong(), 1234);  // db request id
    BOOST_CHECK_EQUAL(result->pop_bool(), true);   // success?

    // Job ID is returned in the message
    job.id = result->pop_ulong();

    auto resultJob = sBundleJob::getById(job.id, onlineCluster->getName(), bundleHash);
    BOOST_CHECK_EQUAL(resultJob.equals(job), true);

    // Save again, error should be set because the bundle hash is incorrect
    job.content = "new test content";
    msg         = Message(DB_BUNDLE_CREATE_OR_UPDATE_JOB);
    msg.push_ulong(12345);  // db request id
    msg.push_string(generateUUID());
    job.toMessage(msg);
    onlineCluster->handleMessage(msg);

    // Get the result and verify success
    result = runCluster();

    BOOST_CHECK_EQUAL(result->pop_ulong(), 12345);  // db request id
    BOOST_CHECK_EQUAL(result->pop_bool(), false);   // success?
}

BOOST_AUTO_TEST_CASE(test_db_bundle_job_get_by_id_success_job_exists)
{
    auto bundleHash = generateUUID();

    // Create a Cluster Job record
    sBundleJob job{.id = 0, .content = "test content"};
    job.save(onlineCluster->getName(), bundleHash);

    // Try to fetch it
    Message msg(DB_BUNDLE_GET_JOB_BY_ID);
    msg.push_ulong(1234);  // db request id
    msg.push_string(bundleHash);
    msg.push_ulong(job.id);
    onlineCluster->handleMessage(msg);

    // Get the result and verify success
    auto result = runCluster();

    BOOST_CHECK_EQUAL(result->pop_ulong(), 1234);  // db request id
    BOOST_CHECK_EQUAL(result->pop_bool(), true);   // success?

    auto resultJob = sBundleJob::fromMessage(*result);
    BOOST_CHECK_EQUAL(resultJob.equals(job), true);
}

BOOST_AUTO_TEST_CASE(test_db_bundle_job_get_by_id_error_job_not_exist)
{
    auto bundleHash = generateUUID();

    // Create a Cluster Job record, but don't save it
    sBundleJob job;

    // Try to fetch it
    Message msg(DB_BUNDLE_GET_JOB_BY_ID);
    msg.push_ulong(1234);  // db request id
    msg.push_string(bundleHash);
    msg.push_ulong(4321);
    onlineCluster->handleMessage(msg);

    // Get the result and verify success
    auto result = runCluster();

    BOOST_CHECK_EQUAL(result->pop_ulong(), 1234);  // db request id
    BOOST_CHECK_EQUAL(result->pop_bool(), false);  // success?
}

BOOST_AUTO_TEST_CASE(test_db_bundle_job_delete_success)
{
    auto bundleHash = generateUUID();

    // Create a Cluster Job record
    sBundleJob job{.id = 0, .content = "test content"};
    job.save(onlineCluster->getName(), bundleHash);

    // Try to delete it
    Message msg(DB_BUNDLE_DELETE_JOB);
    msg.push_ulong(1234);  // db request id
    msg.push_string(bundleHash);
    msg.push_ulong(job.id);
    onlineCluster->handleMessage(msg);

    // Get the result and verify success
    auto result = runCluster();

    BOOST_CHECK_EQUAL(result->pop_ulong(), 1234);  // db request id
    BOOST_CHECK_EQUAL(result->pop_bool(), true);   // success?

    // Job should not exist
    BOOST_CHECK_THROW(sBundleJob::getById(job.id, onlineCluster->getName(), bundleHash), std::runtime_error);
}

BOOST_AUTO_TEST_CASE(test_db_bundle_job_delete_error)
{
    auto bundleHash = generateUUID();

    // Create a Cluster Job record
    sBundleJob job{.id = 0, .content = "test content"};
    job.save(onlineCluster->getName(), bundleHash);

    // Try to delete it with a different bundleHash
    Message msg(DB_BUNDLE_DELETE_JOB);
    msg.push_ulong(1234);  // db request id
    msg.push_string(generateUUID());
    msg.push_ulong(job.id);
    onlineCluster->handleMessage(msg);

    // Get the result and verify success
    auto result = runCluster();

    BOOST_CHECK_EQUAL(result->pop_ulong(), 1234);  // db request id
    BOOST_CHECK_EQUAL(result->pop_bool(), false);  // success?
}
BOOST_AUTO_TEST_SUITE_END()

// NOLINTEND(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)