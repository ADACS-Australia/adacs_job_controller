//
// Created by lewis on 10/5/22.
//

#ifndef GWCLOUD_JOB_SERVER_S_CLUSTER_JOB_STATUS_H
#define GWCLOUD_JOB_SERVER_S_CLUSTER_JOB_STATUS_H

#include "../Lib/Messaging/Message.h"
#include "../Lib/jobserver_schema.h"
#include "MySqlConnector.h"
#include "sClusterJob.h"
#include <cstdint>
#include <string>
#include <thread>
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

    // NOLINTNEXTLINE(bugprone-easily-swappable-parameters)
    static auto getJobStatusByJobIdAndWhat(uint64_t jobId, const std::string& what, const std::string& cluster) {
        auto _database = MySqlConnector();
        schema::JobserverClusterjobstatus _jobStatusTable;

        // Check that a Cluster Job record really exists with the provided job id and cluster
        auto job = sClusterJob::getById(jobId, cluster);
        if (job.id != jobId) {
            throw std::runtime_error("ClusterJob with provided ID did not exist for the provided cluster");
        }

        auto statusResults = _database->operator()(
                select(all_of(_jobStatusTable))
                        .from(_jobStatusTable)
                        .where(
                                _jobStatusTable.jobId == jobId
                                and _jobStatusTable.what == what
                        )
        );

        // Parse the objects
        std::vector<sClusterJobStatus> vStatus;
        for (const auto &record: statusResults) {
            vStatus.push_back(sClusterJobStatus::fromDb(record));
        }

        return vStatus;
    }

    static auto getJobStatusByJobId(uint64_t jobId, const std::string& cluster) {
        auto _database = MySqlConnector();
        schema::JobserverClusterjobstatus _jobStatusTable;

        // Check that a Cluster Job record really exists with the provided job id and cluster
        auto job = sClusterJob::getById(jobId, cluster);
        if (job.id != jobId) {
            throw std::runtime_error("ClusterJob with provided ID did not exist for the provided cluster");
        }

        auto statusResults = _database->operator()(
                select(all_of(_jobStatusTable))
                        .from(_jobStatusTable)
                        .where(
                                _jobStatusTable.jobId == jobId
                        )
        );

        // Parse the objects
        std::vector<sClusterJobStatus> vStatus;
        for (const auto &record: statusResults) {
            vStatus.push_back(sClusterJobStatus::fromDb(record));
        }

        return vStatus;
    }

    static void deleteByIdList(const std::vector<uint64_t>& ids, const std::string& cluster) {
        auto _database = MySqlConnector();
        schema::JobserverClusterjobstatus _jobStatusTable;
        schema::JobserverClusterjob _jobTable;

        // Remove any records where the id is in "ids", and the job id is in any jobs which have the same cluster
        _database->run(
                remove_from(_jobStatusTable)
                        .where(
                                _jobStatusTable.id.in(sqlpp::value_list(ids))
                                and _jobStatusTable.jobId.in(
                                        select(_jobTable.id).from(_jobTable).where(_jobTable.cluster == cluster)
                                )
                        )
        );
    }

    void save(const std::string& cluster) {
        auto _database = MySqlConnector();
        schema::JobserverClusterjobstatus _jobStatusTable;

        // Check that a Cluster Job record really exists with the provided job id and cluster
        auto job = sClusterJob::getById(jobId, cluster);
        if (job.id != jobId) {
            throw std::runtime_error("ClusterJob with provided ID did not exist for the provided cluster");
        }

        if (id != 0) {
            // Update the record
            _database->run(
                    update(_jobStatusTable)
                            .set(
                                    _jobStatusTable.jobId = jobId,
                                    _jobStatusTable.what = what,
                                    _jobStatusTable.state = state
                            )
                            .where(
                                    _jobStatusTable.id == static_cast<uint64_t>(id)
                            )
            );
        } else {
            // Create the record
            id = _database->run(
                    insert_into(_jobStatusTable)
                            .set(
                                    _jobStatusTable.jobId = jobId,
                                    _jobStatusTable.what = what,
                                    _jobStatusTable.state = state
                            )
            );
        }
    }

    uint64_t id = 0;
    uint64_t jobId = 0;
    std::string what;
    uint32_t state = 0;
};

#endif //GWCLOUD_JOB_SERVER_S_CLUSTER_JOB_STATUS_H
