//
// Created by lewis on 10/8/22.
//

import settings;

#include "ClusterDB.h"
#include "../Cluster/Cluster.h"
#include "sBundleJob.h"
#include "sClusterJob.h"
#include "sClusterJobStatus.h"

auto ClusterDB::maybeHandleClusterDBMessage(Message &message, const std::shared_ptr<Cluster> &pCluster) -> bool {
    switch (message.getId()) {
        // Get ClusterJob by Job ID
        case DB_JOB_GET_BY_JOB_ID:
            getClusterJobByJobId(message, pCluster);
            return true;

        // Get ClusterJob by ID
        case DB_JOB_GET_BY_ID:
            getClusterJobById(message, pCluster);
            return true;

        // Get all running jobs for a cluster
        case DB_JOB_GET_RUNNING_JOBS:
            getRunningClusterJobs(message, pCluster);
            return true;

        // Delete a Cluster Job by ID
        case DB_JOB_DELETE:
            deleteClusterJob(message, pCluster);
            return true;

        // Save or update a Cluster Job
        case DB_JOB_SAVE:
            saveClusterJob(message, pCluster);
            return true;

        // Get Cluster Job Status by Job ID and "what"
        case DB_JOBSTATUS_GET_BY_JOB_ID_AND_WHAT:
            getClusterJobStatusByJobIdAndWhat(message, pCluster);
            return true;

        // Get Cluster Job Status by Job ID
        case DB_JOBSTATUS_GET_BY_JOB_ID:
            getClusterJobStatusByJobId(message, pCluster);
            return true;

        // Delete Cluster Job Status by ID list
        case DB_JOBSTATUS_DELETE_BY_ID_LIST:
            deleteClusterJobStatusByIdList(message, pCluster);
            return true;

        // Save or update a Cluster Job Status
        case DB_JOBSTATUS_SAVE:
            saveClusterJobStatus(message, pCluster);
            return true;

        // Create or update a Bundle Job
        case DB_BUNDLE_CREATE_OR_UPDATE_JOB:
            createOrUpdateBundleJob(message, pCluster);
            return true;

        // Get a Bundle Job by ID
        case DB_BUNDLE_GET_JOB_BY_ID:
            getBundleJobById(message, pCluster);
            return true;

        // Delete a Bundle Job by ID
        case DB_BUNDLE_DELETE_JOB:
            deleteBundleJobById(message, pCluster);
            return true;
    }

    return false;
}

void ClusterDB::saveClusterJobStatus(Message &message, const std::shared_ptr<Cluster> &pCluster) {
    auto result = prepareResult(message, pCluster);

    try {
        auto status = sClusterJobStatus::fromMessage(message);
        status.save(pCluster->getName());

        // Success
        result.push_bool(true);
        result.push_ulong(status.id);
    } catch (std::exception &except) {
        // An error occurred, notify the client
        result.push_bool(false);
    }

    pCluster->sendMessage(result);
}

void ClusterDB::deleteClusterJobStatusByIdList(Message &message, const std::shared_ptr<Cluster> &pCluster) {
    auto result = prepareResult(message, pCluster);

    try {
        std::vector<uint64_t> ids;
        auto count = message.pop_uint();
        for (uint32_t index = 0; index < count; index++) {
            ids.push_back(message.pop_ulong());
        }

        sClusterJobStatus::deleteByIdList(ids, pCluster->getName());

        // Success
        result.push_bool(true);
    } catch (std::exception &except) {
        // An error occurred, notify the client
        result.push_bool(false);
    }

    pCluster->sendMessage(result);
}

void ClusterDB::getClusterJobStatusByJobId(Message &message, const std::shared_ptr<Cluster> &pCluster) {
    auto result = prepareResult(message, pCluster);

    try {
        auto statuses = sClusterJobStatus::getJobStatusByJobId(message.pop_ulong(), pCluster->getName());

        // Success
        result.push_bool(true);
        result.push_uint(statuses.size());
        for (const auto &status: statuses) {
            status.toMessage(result);
        }
    } catch (std::exception &except) {
        // An error occurred, notify the client
        result.push_bool(false);
    }

    pCluster->sendMessage(result);
}

void ClusterDB::getClusterJobStatusByJobIdAndWhat(Message &message, const std::shared_ptr<Cluster> &pCluster) {
    auto result = prepareResult(message, pCluster);

    try {
        auto jobId = message.pop_ulong();
        auto what = message.pop_string();
        auto statuses = sClusterJobStatus::getJobStatusByJobIdAndWhat(jobId, what, pCluster->getName());

        // Success
        result.push_bool(true);
        result.push_uint(statuses.size());
        for (const auto &status: statuses) {
            status.toMessage(result);
        }
    } catch (std::exception &except) {
        // An error occurred, notify the client
        result.push_bool(false);
    }

    pCluster->sendMessage(result);
}

void ClusterDB::saveClusterJob(Message &message, const std::shared_ptr<Cluster> &pCluster) {
    auto result = prepareResult(message, pCluster);

    try {
        auto job = sClusterJob::fromMessage(message);
        job.save(pCluster->getName());

        // Success
        result.push_bool(true);
        result.push_ulong(job.id);
    } catch (std::exception &except) {
        // An error occurred, notify the client
        result.push_bool(false);
    }

    pCluster->sendMessage(result);
}

void ClusterDB::deleteClusterJob(Message &message, const std::shared_ptr<Cluster> &pCluster) {
    auto result = prepareResult(message, pCluster);

    try {
        sClusterJob job = {
                .id = message.pop_ulong()
        };
        job._delete(pCluster->getName());

        // Success
        result.push_bool(true);
    } catch (std::exception &except) {
        // An error occurred, notify the client
        result.push_bool(false);
    }

    pCluster->sendMessage(result);
}

void ClusterDB::getRunningClusterJobs(Message &message, const std::shared_ptr<Cluster> &pCluster) {
    auto result = prepareResult(message, pCluster);

    try {
        auto jobs = sClusterJob::getRunningJobs(pCluster->getName());

        // Success
        result.push_bool(true);
        result.push_uint(jobs.size());
        for (const auto &job: jobs) {
            job.toMessage(result);
        }
    } catch (std::exception &except) {
        // An error occurred, notify the client
        result.push_bool(false);
    }

    pCluster->sendMessage(result);
}

void ClusterDB::getClusterJobById(Message &message, const std::shared_ptr<Cluster> &pCluster) {
    auto result = prepareResult(message, pCluster);

    try {
        auto job = sClusterJob::getById(message.pop_ulong(), pCluster->getName());

        // Success
        result.push_bool(true);
        result.push_uint(1);
        job.toMessage(result);
    } catch (std::exception &except) {
        // An error occurred, notify the client
        result.push_bool(false);
    }

    pCluster->sendMessage(result);
}

void ClusterDB::getClusterJobByJobId(Message &message, const std::shared_ptr<Cluster> &pCluster) {
    auto result = prepareResult(message, pCluster);

    try {
        auto job = sClusterJob::getOrCreateByJobId(message.pop_ulong(), pCluster->getName());

        // Success
        result.push_bool(true);
        result.push_uint(1);
        job.toMessage(result);
    } catch (std::exception &except) {
        // An error occurred, notify the client
        result.push_bool(false);
    }

    pCluster->sendMessage(result);
}

auto ClusterDB::prepareResult(Message &message, const std::shared_ptr<Cluster> &pCluster) -> Message {
    auto dbRequestId = message.pop_ulong();

    auto result = Message(DB_RESPONSE, Message::Medium, "database_" + pCluster->getName());
    result.push_ulong(dbRequestId);
    return result;
}

void ClusterDB::createOrUpdateBundleJob(Message &message, const std::shared_ptr<Cluster> &pCluster) {
    auto result = prepareResult(message, pCluster);
    auto bundleHash = message.pop_string();

    try {
        auto job = sBundleJob::fromMessage(message);
        job.save(pCluster->getName(), bundleHash);

        // Success
        result.push_bool(true);
        result.push_ulong(job.id);
    } catch (std::exception &except) {
        // An error occurred, notify the client
        result.push_bool(false);
    }

    pCluster->sendMessage(result);
}

void ClusterDB::getBundleJobById(Message &message, const std::shared_ptr<Cluster> &pCluster) {
    auto result = prepareResult(message, pCluster);
    auto bundleHash = message.pop_string();

    try {
        auto job = sBundleJob::getById(message.pop_ulong(), pCluster->getName(), bundleHash);

        // Success
        result.push_bool(true);
        job.toMessage(result);
    } catch (std::exception &except) {
        // An error occurred, notify the client
        result.push_bool(false);
    }

    pCluster->sendMessage(result);
}

void ClusterDB::deleteBundleJobById(Message &message, const std::shared_ptr<Cluster> &pCluster) {
    auto result = prepareResult(message, pCluster);
    auto bundleHash = message.pop_string();

    try {
        sBundleJob job = {
                .id = message.pop_ulong()
        };
        job._delete(pCluster->getName(), bundleHash);

        // Success
        result.push_bool(true);
    } catch (std::exception &except) {
        // An error occurred, notify the client
        result.push_bool(false);
    }

    pCluster->sendMessage(result);
}