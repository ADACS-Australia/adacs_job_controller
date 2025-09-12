//
// Created by lewis on 10/5/22.
//

#ifndef GWCLOUD_JOB_SERVER_S_CLUSTER_JOB_H
#define GWCLOUD_JOB_SERVER_S_CLUSTER_JOB_H

import Message;
#include <cstdint>
#include <string>
#include <vector>


struct sClusterJob {
    auto equals(sClusterJob& other) const -> bool {
        return id == other.id
            and jobId == other.jobId
            and schedulerId == other.schedulerId
            and submitting == other.submitting
            and submittingCount == other.submittingCount
            and bundleHash == other.bundleHash
            and workingDirectory == other.workingDirectory
            and running == other.running
            and deleting == other.deleting
            and deleted == other.deleted;
    }

    static auto fromRecord(auto record) -> sClusterJob {
        return {
                .id = static_cast<uint64_t>(record->id),
                .jobId = static_cast<uint64_t>(record->jobId),
                .schedulerId = static_cast<uint64_t>(record->schedulerId),
                .submitting = static_cast<uint32_t>(record->submitting) == 1,
                .submittingCount = static_cast<uint32_t>(record->submittingCount),
                .bundleHash = record->bundleHash,
                .workingDirectory = record->workingDirectory,
                .running = static_cast<uint32_t>(record->running) == 1,
                .deleting = static_cast<uint32_t>(record->deleting) == 1,
                .deleted = static_cast<uint32_t>(record->deleted) == 1
        };
    }

    void toMessage(Message& message) const {
        message.push_ulong(id);
        message.push_ulong(jobId);
        message.push_ulong(schedulerId);
        message.push_bool(submitting);
        message.push_uint(submittingCount);
        message.push_string(bundleHash);
        message.push_string(workingDirectory);
        message.push_bool(running);
        message.push_bool(deleting);
        message.push_bool(deleted);
    }

    static auto fromMessage(Message& message) -> sClusterJob {
        return {
                .id = message.pop_ulong(),
                .jobId = message.pop_ulong(),
                .schedulerId = message.pop_ulong(),
                .submitting = message.pop_bool(),
                .submittingCount = message.pop_uint(),
                .bundleHash = message.pop_string(),
                .workingDirectory = message.pop_string(),
                .running = message.pop_bool(),
                .deleting = message.pop_bool(),
                .deleted = message.pop_bool()
        };
    }

    // Database methods - implemented in sClusterJob.cpp
    static auto getOrCreateByJobId(uint64_t jobId, const std::string& cluster) -> sClusterJob;
    static auto getRunningJobs(const std::string& cluster) -> std::vector<sClusterJob>;
    void _delete(const std::string& cluster) const;
    void save(const std::string& cluster);
    static auto getById(uint64_t identifier, const std::string& cluster) -> sClusterJob;

    uint64_t id = 0;
    uint64_t jobId = 0;
    uint64_t schedulerId = 0;
    bool submitting = false;
    uint32_t submittingCount = 0;
    std::string bundleHash;
    std::string workingDirectory;
    bool running = true;
    bool deleting = false;
    bool deleted = false;
};


#endif //GWCLOUD_JOB_SERVER_S_CLUSTER_JOB_H
