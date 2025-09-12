//
// Created by lewis on 10/5/22.
//

#include "MySqlConnector.h"
#include "sClusterJob.h"
#include "../Lib/jobserver_schema.h"
#include <sqlpp11/sqlpp11.h>

using namespace sqlpp;

sClusterJob sClusterJob::getOrCreateByJobId(uint64_t jobId, const std::string& cluster) {
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

    return sClusterJob{};
}

std::vector<sClusterJob> sClusterJob::getRunningJobs(const std::string& cluster) {
    auto _database = MySqlConnector();
    schema::JobserverClusterjob _jobTable;

    auto jobResults =
            _database->run(
                    select(all_of(_jobTable))
                            .from(_jobTable)
                            .where(
                                    _jobTable.running == 1
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

void sClusterJob::_delete(const std::string& cluster) const {
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

void sClusterJob::save(const std::string& cluster) {
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
                                _jobTable.running = running ? 1 : 0,
                                _jobTable.deleting = deleting ? 1 : 0,
                                _jobTable.deleted = deleted ? 1 : 0
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
                                _jobTable.running = running ? 1 : 0,
                                _jobTable.deleting = deleting ? 1 : 0,
                                _jobTable.deleted = deleted ? 1 : 0,
                                _jobTable.cluster = cluster
                        )
        );
    }
}

sClusterJob sClusterJob::getById(uint64_t identifier, const std::string& cluster) {
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

    return sClusterJob{};
}
