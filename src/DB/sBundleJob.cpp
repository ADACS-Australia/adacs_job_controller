//
// Created by lewis on 10/5/22.
//

#include "MySqlConnector.h"
#include "../Lib/jobserver_schema.h"
#include <sqlpp11/sqlpp11.h>
#include "sBundleJob.h"

using namespace sqlpp;

sBundleJob sBundleJob::getById(uint64_t jobId, const std::string& cluster, const std::string& bundleHash) {
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

void sBundleJob::_delete(const std::string& cluster, const std::string& bundleHash) const {
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

void sBundleJob::save(const std::string& cluster, const std::string& bundleHash) {
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
