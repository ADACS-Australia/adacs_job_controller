//
// Created by lewis on 10/5/22.
//

#ifndef GWCLOUD_JOB_SERVER_S_CLUSTER_JOB_H
#define GWCLOUD_JOB_SERVER_S_CLUSTER_JOB_H

#include "../Lib/Messaging/Message.h"
#include "../Lib/jobserver_schema.h"
#include "MySqlConnector.h"
#include <cstdint>
#include <string>
#include <thread>


struct sClusterJob {
    auto equals(sClusterJob& other) const -> bool {
        return id == other.id
            and jobId == other.jobId
            and schedulerId == other.schedulerId
            and submitting == other.submitting
            and submittingCount == other.submittingCount
            and bundleHash == other.bundleHash
            and workingDirectory == other.workingDirectory
            and queued == other.queued
            and params == other.params
            and running == other.running;
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
                .queued = static_cast<uint32_t>(record->queued) == 1,
                .params = record->params,
                .running = static_cast<uint32_t>(record->running) == 1
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
        message.push_bool(queued);
        message.push_string(params);
        message.push_bool(running);
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
                .queued = message.pop_bool(),
                .params = message.pop_string(),
                .running = message.pop_bool()
        };
    }

    static auto getOrCreateByJobId(uint64_t jobId, const std::string& cluster) -> sClusterJob {
        auto _database = MySqlConnector();
        schema::JobserverClusterjob _jobTable;

        auto jobResults =
                _database->run(
                        select(all_of(_jobTable))
                                .from(_jobTable)
                                .where(
                                        _jobTable.jobId == static_cast<uint64_t>(jobId)
                                        and _jobTable.cluster == cluster
                                )
                );

        if (!jobResults.empty()) {
            return fromRecord(&jobResults.front());
        }

        return {};
    }

    static auto getRunningJobs(const std::string& cluster) -> std::vector<sClusterJob> {
        auto _database = MySqlConnector();
        schema::JobserverClusterjob _jobTable;

        auto jobResults =
                _database->run(
                        select(all_of(_jobTable))
                                .from(_jobTable)
                                .where(
                                        _jobTable.running == 1
                                        and _jobTable.queued == 0
                                        and _jobTable.jobId != 0
                                        and _jobTable.submitting == 0
                                        and _jobTable.cluster == cluster
                                )
                );

        std::vector<sClusterJob> jobs;
        for (const auto& job : jobResults) {
            jobs.push_back(fromRecord(&job));
        }

        return jobs;
    }

    void _delete(const std::string& cluster) const {
        auto _database = MySqlConnector();
        schema::JobserverClusterjob _jobTable;

        _database->run(
                remove_from(_jobTable)
                        .where(
                                _jobTable.id == static_cast<uint64_t>(id)
                                and _jobTable.cluster == cluster
                        )
        );
    }

    void save(const std::string& cluster) {
        auto _database = MySqlConnector();
        schema::JobserverClusterjob _jobTable;

        if (id != 0) {
            // Update the record
            _database->run(
                    update(_jobTable)
                            .set(
                                    _jobTable.jobId = jobId,
                                    _jobTable.schedulerId = schedulerId,
                                    _jobTable.submitting = submitting ? 1 : 0,
                                    _jobTable.submittingCount = submittingCount,
                                    _jobTable.bundleHash = bundleHash,
                                    _jobTable.workingDirectory = workingDirectory,
                                    _jobTable.queued = queued ? 1 : 0,
                                    _jobTable.params = params,
                                    _jobTable.running = running ? 1 : 0
                            )
                            .where(
                                    _jobTable.id == static_cast<uint64_t>(id)
                                    and _jobTable.cluster == cluster
                            )
            );
        } else {
            // Create the record
            id = _database->run(
                    insert_into(_jobTable)
                            .set(
                                    _jobTable.jobId = jobId,
                                    _jobTable.schedulerId = schedulerId,
                                    _jobTable.submitting = submitting ? 1 : 0,
                                    _jobTable.submittingCount = submittingCount,
                                    _jobTable.bundleHash = bundleHash,
                                    _jobTable.workingDirectory = workingDirectory,
                                    _jobTable.queued = queued ? 1 : 0,
                                    _jobTable.params = params,
                                    _jobTable.running = running ? 1 : 0,
                                    _jobTable.cluster = cluster
                            )
            );
        }
    }

    static auto getById(uint64_t identifier, const std::string& cluster) -> sClusterJob {
        auto _database = MySqlConnector();
        schema::JobserverClusterjob _jobTable;

        auto jobResults =
                _database->run(
                        select(all_of(_jobTable))
                                .from(_jobTable)
                                .where(
                                        _jobTable.id == static_cast<uint64_t>(identifier)
                                        and _jobTable.cluster == cluster
                                )
                );

        if (!jobResults.empty()) {
            return fromRecord(&jobResults.front());
        }

        return {};
    }

    // NOLINTBEGIN(misc-non-private-member-variables-in-classes)
    uint64_t id = 0;
    uint64_t jobId = 0;
    uint64_t schedulerId = 0;
    bool submitting = false;
    uint32_t submittingCount = 0;
    std::string bundleHash;
    std::string workingDirectory;
    bool queued = false;
    std::string params;
    bool running = false;
    // NOLINTEND(misc-non-private-member-variables-in-classes)
};


#endif //GWCLOUD_JOB_SERVER_S_CLUSTER_JOB_H
