//
// Created by lewis on 10/5/22.
//

#ifndef GWCLOUD_JOB_SERVER_S_CLUSTER_JOB_STATUS_H
#define GWCLOUD_JOB_SERVER_S_CLUSTER_JOB_STATUS_H

import Message;
#include <cstdint>
#include <string>
#include <vector>


struct sClusterJobStatus {
    [[nodiscard]] auto equals(const sClusterJobStatus& other) const -> bool {
        return id == other.id
            and jobId == other.jobId
            and what == other.what
            and state == other.state;
    }

    static auto fromDb(auto &record) -> sClusterJobStatus {
        return {
                .id = static_cast<uint64_t>(record.id),
                .jobId = static_cast<uint64_t>(record.jobId),
                .what = record.what,
                .state = static_cast<uint32_t>(record.state)
        };
    }

    void toMessage(Message& message) const {
        message.push_ulong(id);
        message.push_ulong(jobId);
        message.push_string(what);
        message.push_uint(state);
    }

    static auto fromMessage(Message& message) -> sClusterJobStatus {
        return {
            .id = message.pop_ulong(),
            .jobId = message.pop_ulong(),
            .what = message.pop_string(),
            .state = message.pop_uint()
        };
    }

    // Database methods - implemented in sClusterJobStatus.cpp
    // NOLINTNEXTLINE(bugprone-easily-swappable-parameters)
    static auto getJobStatusByJobIdAndWhat(uint64_t jobId, const std::string& what, const std::string& cluster) -> std::vector<sClusterJobStatus>;
    static auto getJobStatusByJobId(uint64_t jobId, const std::string& cluster) -> std::vector<sClusterJobStatus>;
    static void deleteByIdList(const std::vector<uint64_t>& ids, const std::string& cluster);
    void save(const std::string& cluster);

    uint64_t id = 0;
    uint64_t jobId = 0;
    std::string what;
    uint32_t state = 0;
};

#endif //GWCLOUD_JOB_SERVER_S_CLUSTER_JOB_STATUS_H
