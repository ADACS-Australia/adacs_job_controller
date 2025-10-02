//
// Created by lewis on 10/5/22.
//

module;
#include <iostream>

#include <sqlpp11/mysql/mysql.h>
#include <sqlpp11/sqlpp11.h>

#include "../Lib/shims/sqlpp_shim.h"

export module sBundleJob;

import Message;
import MySqlConnector;
import jobserver_schema;

export struct sBundleJob
{
    [[nodiscard]] auto equals(const sBundleJob& other) const -> bool
    {
        return id == other.id and content == other.content;
    }

    static auto fromDb(auto& record) -> sBundleJob
    {
        return {.id = static_cast<uint64_t>(record.id), .content = record.content};
    }

    void toMessage(Message& message) const
    {
        message.push_ulong(id);
        message.push_string(content);
    }

    static auto fromMessage(Message& message) -> sBundleJob
    {
        return {.id = message.pop_ulong(), .content = message.pop_string()};
    }

    // Database methods
    // NOLINTNEXTLINE(bugprone-easily-swappable-parameters)
    static auto getById(uint64_t jobId, const std::string& cluster, const std::string& bundleHash) -> sBundleJob
    {
        auto _database = MySqlConnector();
        const schema::JobserverBundlejob _bundleJobTable;

        auto jobResults =
            _database->operator()(sqlpp::select(sqlpp::all_of(_bundleJobTable))
                                      .from(_bundleJobTable)
                                      .where(_bundleJobTable.id == jobId and _bundleJobTable.cluster == cluster and
                                             _bundleJobTable.bundleHash == bundleHash));

        // If no records are found, raise an exception
        if (jobResults.empty())
        {
            throw std::runtime_error("No Bundle Job records found with provided ID.");
        }

        // Parse the result
        return sBundleJob::fromDb(jobResults.front());
    }

    void _delete(const std::string& cluster, const std::string& bundleHash) const
    {
        auto _database = MySqlConnector();
        const schema::JobserverBundlejob _bundleJobTable;

        // Check that a Bundle Job with the provided ID really exists for the provided cluster and bundleHash
        // This function will raise if a job is not found
        sBundleJob::getById(id, cluster, bundleHash);

        auto result = _database->run(sqlpp::remove_from(_bundleJobTable)
                                         .where(_bundleJobTable.id == id and _bundleJobTable.cluster == cluster and
                                                _bundleJobTable.bundleHash == bundleHash));
        if (result != 1)
        {
            std::cerr << "DB: Failed to delete bundle job with id " << id << " for cluster " << cluster
                      << " and bundleHash " << bundleHash << ". Expected 1 row affected, got " << result << ".\n";
        }
    }

    void save(const std::string& cluster, const std::string& bundleHash)
    {
        auto _database = MySqlConnector();
        const schema::JobserverBundlejob _bundleJobTable;

        if (id != 0)
        {
            // Check that a Bundle Job with the provided ID really exists for the provided cluster and bundleHash
            // This function will raise if a job is not found
            sBundleJob::getById(id, cluster, bundleHash);

            // Update the record
            auto result = _database->run(sqlpp::update(_bundleJobTable)
                                             .set(_bundleJobTable.content = content)
                                             .where(_bundleJobTable.id == id and _bundleJobTable.cluster == cluster and
                                                    _bundleJobTable.bundleHash == bundleHash));
            if (result != 1)
            {
                std::cerr << "DB: Failed to update bundle job with id " << id << " for cluster " << cluster
                          << " and bundleHash " << bundleHash << ". Expected 1 row affected, got " << result << ".\n";
            }
        }
        else
        {
            // Create the record
            id = _database->run(sqlpp::insert_into(_bundleJobTable)
                                    .set(_bundleJobTable.content    = content,
                                         _bundleJobTable.cluster    = cluster,
                                         _bundleJobTable.bundleHash = bundleHash));
        }
    }

    uint64_t id = 0;
    std::string content;
};