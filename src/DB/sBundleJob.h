//
// Created by lewis on 10/5/22.
//

#ifndef GWCLOUD_JOB_SERVER_S_BUNDLE_JOB_H
#define GWCLOUD_JOB_SERVER_S_BUNDLE_JOB_H

#include "../Lib/Messaging/Message.h"
#include "../Lib/jobserver_schema.h"
#include "MySqlConnector.h"
#include "sClusterJob.h"
#include <cstdint>
#include <string>
#include <thread>
#include <vector>


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

    // NOLINTNEXTLINE(bugprone-easily-swappable-parameters)
    static auto getById(uint64_t jobId, const std::string& cluster, const std::string& bundleHash) {
        auto _database = MySqlConnector();
        schema::JobserverBundlejob _bundleJobTable;

        auto jobResults = _database->operator()(
                select(all_of(_bundleJobTable))
                        .from(_bundleJobTable)
                        .where(
                                _bundleJobTable.id == jobId
                                and _bundleJobTable.cluster == cluster
                                and _bundleJobTable.bundleHash == bundleHash
                        )
        );

        // If no records are found, raise an exception
        if (jobResults.empty()) {
            throw std::runtime_error("No Bundle Job records found with provided ID.");
        }

        // Parse the result
        return sBundleJob::fromDb(jobResults.front());
    }

    void _delete(const std::string& cluster, const std::string& bundleHash) const {
        auto _database = MySqlConnector();
        schema::JobserverBundlejob _bundleJobTable;

        // Check that a Bundle Job with the provided ID really exists for the provided cluster and bundleHash
        // This function will raise if a job is not found
        sBundleJob::getById(id, cluster, bundleHash);

        _database->run(
                remove_from(_bundleJobTable)
                        .where(
                                _bundleJobTable.id == static_cast<uint64_t>(id)
                                and _bundleJobTable.cluster == cluster
                                and _bundleJobTable.bundleHash == bundleHash
                        )
        );
    }

    void save(const std::string& cluster, const std::string& bundleHash) {
        auto _database = MySqlConnector();
        schema::JobserverBundlejob _bundleJobTable;

        if (id != 0) {
            // Check that a Bundle Job with the provided ID really exists for the provided cluster and bundleHash
            // This function will raise if a job is not found
            sBundleJob::getById(id, cluster, bundleHash);

            // Update the record
            _database->run(
                    update(_bundleJobTable)
                            .set(
                                    _bundleJobTable.content = content
                            )
                            .where(
                                    _bundleJobTable.id == static_cast<uint64_t>(id)
                                    and _bundleJobTable.cluster == cluster
                                    and _bundleJobTable.bundleHash == bundleHash
                            )
            );
        } else {
            // Create the record
            id = _database->run(
                    insert_into(_bundleJobTable)
                            .set(
                                    _bundleJobTable.content = content,
                                    _bundleJobTable.cluster = cluster,
                                    _bundleJobTable.bundleHash = bundleHash
                            )
            );
        }
    }

    // NOLINTBEGIN(misc-non-private-member-variables-in-classes)
    uint64_t id;
    std::string content;
    // NOLINTEND(misc-non-private-member-variables-in-classes)
};

#endif //GWCLOUD_JOB_SERVER_S_BUNDLE_JOB_H
