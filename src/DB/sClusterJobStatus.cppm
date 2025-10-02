//
// Created by lewis on 10/5/22.
//

module;
#include <iostream>

#include <sqlpp11/mysql/mysql.h>
#include <sqlpp11/sqlpp11.h>

#include "../Lib/shims/sqlpp_shim.h"
import jobserver_schema;

export module sClusterJobStatus;

import Message;
import sClusterJob;
import MySqlConnector;

export struct sClusterJobStatus
{
    [[nodiscard]] auto equals(const sClusterJobStatus& other) const -> bool
    {
        return id == other.id and jobId == other.jobId and what == other.what and state == other.state;
    }

    static auto fromDb(auto& record) -> sClusterJobStatus
    {
        return {.id    = static_cast<uint64_t>(record.id),
                .jobId = static_cast<uint64_t>(record.jobId),
                .what  = record.what,
                .state = static_cast<uint32_t>(record.state)};
    }

    void toMessage(Message& message) const
    {
        message.push_ulong(id);
        message.push_ulong(jobId);
        message.push_string(what);
        message.push_uint(state);
    }

    static auto fromMessage(Message& message) -> sClusterJobStatus
    {
        return {.id    = message.pop_ulong(),
                .jobId = message.pop_ulong(),
                .what  = message.pop_string(),
                .state = message.pop_uint()};
    }

    // Database methods
    // NOLINTNEXTLINE(bugprone-easily-swappable-parameters)
    static auto getJobStatusByJobIdAndWhat(uint64_t jobId,
                                           const std::string& what,
                                           const std::string& cluster) -> std::vector<sClusterJobStatus>
    {
        auto _database = MySqlConnector();
        const schema::JobserverClusterjobstatus _jobStatusTable;

        // Check that a Cluster Job record really exists with the provided job id and cluster
        auto job = sClusterJob::getById(jobId, cluster);
        if (job.id != jobId)
        {
            throw std::runtime_error("ClusterJob with provided ID did not exist for the provided cluster");
        }

        auto statusResults =
            _database->operator()(sqlpp::select(sqlpp::all_of(_jobStatusTable))
                                      .from(_jobStatusTable)
                                      .where(_jobStatusTable.jobId == jobId and _jobStatusTable.what == what));

        // Parse the objects
        std::vector<sClusterJobStatus> vStatus;
        for (const auto& record : statusResults)
        {
            vStatus.push_back(sClusterJobStatus::fromDb(record));
        }

        return vStatus;
    }

    static auto getJobStatusByJobId(uint64_t jobId, const std::string& cluster) -> std::vector<sClusterJobStatus>
    {
        auto _database = MySqlConnector();
        const schema::JobserverClusterjobstatus _jobStatusTable;

        // Check that a Cluster Job record really exists with the provided job id and cluster
        auto job = sClusterJob::getById(jobId, cluster);
        if (job.id != jobId)
        {
            throw std::runtime_error("ClusterJob with provided ID did not exist for the provided cluster");
        }

        auto statusResults = _database->operator()(
            sqlpp::select(sqlpp::all_of(_jobStatusTable)).from(_jobStatusTable).where(_jobStatusTable.jobId == jobId));

        // Parse the objects
        std::vector<sClusterJobStatus> vStatus;
        for (const auto& record : statusResults)
        {
            vStatus.push_back(sClusterJobStatus::fromDb(record));
        }

        return vStatus;
    }

    static void deleteByIdList(const std::vector<uint64_t>& ids, const std::string& cluster)
    {
        auto _database = MySqlConnector();
        const schema::JobserverClusterjobstatus _jobStatusTable;
        const schema::JobserverClusterjob _jobTable;

        // Remove any records where the id is in "ids", and the job id is in any jobs which have the same cluster
        auto result = _database->run(
            sqlpp::remove_from(_jobStatusTable)
                .where(_jobStatusTable.id.in(sqlpp::value_list(ids)) and
                       _jobStatusTable.jobId.in(
                           sqlpp::select(_jobTable.id).from(_jobTable).where(_jobTable.cluster == cluster))));
        if (result != ids.size())
        {
            std::cerr << "WARNING: DB - Failed to delete cluster job status records for cluster " << cluster
                      << ", expected " << ids.size() << " rows but got " << result << '\n';
        }
    }

    void save(const std::string& cluster)
    {
        auto _database = MySqlConnector();
        const schema::JobserverClusterjobstatus _jobStatusTable;

        // Check that a Cluster Job record really exists with the provided job id and cluster
        auto job = sClusterJob::getById(jobId, cluster);
        if (job.id != jobId)
        {
            throw std::runtime_error("ClusterJob with provided ID did not exist for the provided cluster");
        }

        if (id != 0)
        {
            // Update the record
            auto result = _database->run(
                sqlpp::update(_jobStatusTable)
                    .set(_jobStatusTable.jobId = jobId, _jobStatusTable.what = what, _jobStatusTable.state = state)
                    .where(_jobStatusTable.id == id));
            if (result != 1)
            {
                std::cerr << "WARNING: DB - Failed to update cluster job status with id " << id
                          << ", expected 1 row but got " << result << '\n';
            }
        }
        else
        {
            // Create the record
            id = _database->run(
                sqlpp::insert_into(_jobStatusTable)
                    .set(_jobStatusTable.jobId = jobId, _jobStatusTable.what = what, _jobStatusTable.state = state));
        }
    }

    uint64_t id    = 0;
    uint64_t jobId = 0;
    std::string what;
    uint32_t state = 0;
};
