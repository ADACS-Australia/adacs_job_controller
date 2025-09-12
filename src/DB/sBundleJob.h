//
// Created by lewis on 10/5/22.
//

#ifndef GWCLOUD_JOB_SERVER_S_BUNDLE_JOB_H
#define GWCLOUD_JOB_SERVER_S_BUNDLE_JOB_H

import Message;
#include <cstdint>
#include <string>


struct sBundleJob {
    [[nodiscard]] auto equals(const sBundleJob& other) const -> bool {
        return id == other.id
            and content == other.content;
    }

    static auto fromDb(auto &record) -> sBundleJob {
        return {
                .id = static_cast<uint64_t>(record.id),
                .content = record.content
        };
    }

    void toMessage(Message& message) const {
        message.push_ulong(id);
        message.push_string(content);
    }

    static auto fromMessage(Message& message) -> sBundleJob {
        return {
            .id = message.pop_ulong(),
            .content = message.pop_string()
        };
    }

    // Database methods - implemented in sBundleJob.cpp
    // NOLINTNEXTLINE(bugprone-easily-swappable-parameters)
    static auto getById(uint64_t jobId, const std::string& cluster, const std::string& bundleHash) -> sBundleJob;
    void _delete(const std::string& cluster, const std::string& bundleHash) const;
    void save(const std::string& cluster, const std::string& bundleHash);

    uint64_t id = 0;
    std::string content;
};

#endif //GWCLOUD_JOB_SERVER_S_BUNDLE_JOB_H
