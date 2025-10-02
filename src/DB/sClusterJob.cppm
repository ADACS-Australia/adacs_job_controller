//
// Created by lewis on 10/5/22.
//

module;
#include <iostream>

#include <sqlpp11/mysql/mysql.h>
#include <sqlpp11/sqlpp11.h>
import jobserver_schema;

export module sClusterJob;

import Message;
import MySqlConnector;

export struct sClusterJob
{
    [[nodiscard]] auto equals(const sClusterJob& other) const -> bool
    {
        return id == other.id and jobId == other.jobId and schedulerId == other.schedulerId and
               submitting == other.submitting and submittingCount == other.submittingCount and
               bundleHash == other.bundleHash and workingDirectory == other.workingDirectory and
               running == other.running and deleting == other.deleting and deleted == other.deleted;
    }

    static auto fromRecord(auto record) -> sClusterJob
    {
        return {.id               = static_cast<uint64_t>(record->id),
                .jobId            = static_cast<uint64_t>(record->jobId),
                .schedulerId      = static_cast<uint64_t>(record->schedulerId),
                .submitting       = static_cast<uint32_t>(record->submitting) == 1,
                .submittingCount  = static_cast<uint32_t>(record->submittingCount),
                .bundleHash       = record->bundleHash,
                .workingDirectory = record->workingDirectory,
                .running          = static_cast<uint32_t>(record->running) == 1,
                .deleting         = static_cast<uint32_t>(record->deleting) == 1,
                .deleted          = static_cast<uint32_t>(record->deleted) == 1};
    }

    void toMessage(Message& message) const
    {
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

    static auto fromMessage(Message& message) -> sClusterJob
    {
        return {.id               = message.pop_ulong(),
                .jobId            = message.pop_ulong(),
                .schedulerId      = message.pop_ulong(),
                .submitting       = message.pop_bool(),
                .submittingCount  = message.pop_uint(),
                .bundleHash       = message.pop_string(),
                .workingDirectory = message.pop_string(),
                .running          = message.pop_bool(),
                .deleting         = message.pop_bool(),
                .deleted          = message.pop_bool()};
    }

    // Database methods
    static auto getOrCreateByJobId(uint64_t jobId, const std::string& cluster) -> sClusterJob
    {
        auto _database = MySqlConnector();
        const schema::JobserverClusterjob _jobTable;

        auto jobResults = _database->run(sqlpp::select(sqlpp::all_of(_jobTable))
                                             .from(_jobTable)
                                             .where(_jobTable.jobId == jobId and _jobTable.cluster == cluster));

        if (!jobResults.empty())
        {
            return fromRecord(&jobResults.front());
        }

        return sClusterJob{};
    }

    static auto getRunningJobs(const std::string& cluster) -> std::vector<sClusterJob>
    {
        auto _database = MySqlConnector();
        const schema::JobserverClusterjob _jobTable;

        auto jobResults = _database->run(sqlpp::select(sqlpp::all_of(_jobTable))
                                             .from(_jobTable)
                                             .where(_jobTable.running == 1 and _jobTable.jobId != 0 and
                                                    _jobTable.submitting == 0 and _jobTable.cluster == cluster));

        std::vector<sClusterJob> jobs;
        for (const auto& job : jobResults)
        {
            jobs.push_back(fromRecord(&job));
        }

        return jobs;
    }

    void _delete(const std::string& cluster) const
    {
        auto _database = MySqlConnector();
        const schema::JobserverClusterjob _jobTable;

        auto result =
            _database->run(sqlpp::remove_from(_jobTable).where(_jobTable.id == id and _jobTable.cluster == cluster));
        if (result != 1)
        {
            std::cerr << "WARNING: DB - Failed to delete cluster job with id " << id << " for cluster " << cluster
                      << ", expected 1 row but got " << result << '\n';
        }
    }

    void save(const std::string& cluster)
    {
        auto _database = MySqlConnector();
        const schema::JobserverClusterjob _jobTable;

        if (id != 0)
        {
            // Update the record
            auto result = _database->run(sqlpp::update(_jobTable)
                                             .set(_jobTable.jobId            = jobId,
                                                  _jobTable.schedulerId      = schedulerId,
                                                  _jobTable.submitting       = submitting ? 1 : 0,
                                                  _jobTable.submittingCount  = submittingCount,
                                                  _jobTable.bundleHash       = bundleHash,
                                                  _jobTable.workingDirectory = workingDirectory,
                                                  _jobTable.running          = running ? 1 : 0,
                                                  _jobTable.deleting         = deleting ? 1 : 0,
                                                  _jobTable.deleted          = deleted ? 1 : 0)
                                             .where(_jobTable.id == id and _jobTable.cluster == cluster));
            if (result != 1)
            {
                std::cerr << "WARNING: DB - Failed to update cluster job with id " << id << " for cluster " << cluster
                          << ", expected 1 row but got " << result << '\n';
            }
        }
        else
        {
            // Create the record
            id = _database->run(sqlpp::insert_into(_jobTable).set(_jobTable.jobId            = jobId,
                                                                  _jobTable.schedulerId      = schedulerId,
                                                                  _jobTable.submitting       = submitting ? 1 : 0,
                                                                  _jobTable.submittingCount  = submittingCount,
                                                                  _jobTable.bundleHash       = bundleHash,
                                                                  _jobTable.workingDirectory = workingDirectory,
                                                                  _jobTable.running          = running ? 1 : 0,
                                                                  _jobTable.deleting         = deleting ? 1 : 0,
                                                                  _jobTable.deleted          = deleted ? 1 : 0,
                                                                  _jobTable.cluster          = cluster));
        }
    }

    static auto getById(uint64_t identifier, const std::string& cluster) -> sClusterJob
    {
        auto _database = MySqlConnector();
        const schema::JobserverClusterjob _jobTable;

        auto jobResults = _database->run(sqlpp::select(sqlpp::all_of(_jobTable))
                                             .from(_jobTable)
                                             .where(_jobTable.id == identifier and _jobTable.cluster == cluster));

        if (!jobResults.empty())
        {
            return fromRecord(&jobResults.front());
        }

        return sClusterJob{};
    }

    uint64_t id              = 0;
    uint64_t jobId           = 0;
    uint64_t schedulerId     = 0;
    bool submitting          = false;
    uint32_t submittingCount = 0;
    std::string bundleHash;
    std::string workingDirectory;
    bool running  = true;
    bool deleting = false;
    bool deleted  = false;
};
