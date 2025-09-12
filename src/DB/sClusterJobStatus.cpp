//
// Created by lewis on 10/5/22.
//

#include "MySqlConnector.h"
#include "../Lib/jobserver_schema.h"
#include <sqlpp11/sqlpp11.h>
#include "sClusterJobStatus.h"
#include "sClusterJob.h"

using namespace sqlpp;

std::vector<sClusterJobStatus> sClusterJobStatus::getJobStatusByJobIdAndWhat(uint64_t jobId, const std::string& what, const std::string& cluster) {
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

std::vector<sClusterJobStatus> sClusterJobStatus::getJobStatusByJobId(uint64_t jobId, const std::string& cluster) {
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

void sClusterJobStatus::deleteByIdList(const std::vector<uint64_t>& ids, const std::string& cluster) {
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

void sClusterJobStatus::save(const std::string& cluster) {
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
