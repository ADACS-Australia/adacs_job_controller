//
// Created by lewis on 7/25/22.
//

#ifndef GWCLOUD_JOB_SERVER_DATABASEFIXTURE_H
#define GWCLOUD_JOB_SERVER_DATABASEFIXTURE_H

#include <sqlpp11/sqlpp11.h>
#include "../../Lib/shims/sqlpp_shim.h"
import jobserver_schema;

import MySqlConnector;

struct DatabaseFixture {
    MySqlConnector database;

    schema::JobserverFiledownload fileDownloadTable{};
    schema::JobserverJob jobTable{};
    schema::JobserverJobhistory jobHistoryTable{};
    schema::JobserverClusteruuid jobClusteruuid{};
    schema::JobserverFilelistcache jobFilelistcache{};
    schema::JobserverClusterjob jobClusterjob{};
    schema::JobserverClusterjobstatus jobClusterjobstatus{};

    DatabaseFixture() {
        cleanDatabase();
    }

    // NOLINTNEXTLINE(bugprone-exception-escape)
    ~DatabaseFixture() {
        cleanDatabase();
    }

    DatabaseFixture(DatabaseFixture const&) = delete;
    auto operator =(DatabaseFixture const&) -> DatabaseFixture& = delete;
    DatabaseFixture(DatabaseFixture&&) = delete;
    auto operator=(DatabaseFixture&&) -> DatabaseFixture& = delete;

private:
    void cleanDatabase() const {
        // Sanitize all records from the database
        database->run(remove_from(jobFilelistcache).unconditionally());
        database->run(remove_from(fileDownloadTable).unconditionally());
        database->run(remove_from(jobHistoryTable).unconditionally());
        database->run(remove_from(jobTable).unconditionally());
        database->run(remove_from(jobClusteruuid).unconditionally());
        database->run(remove_from(jobClusterjobstatus).unconditionally());
        database->run(remove_from(jobClusterjob).unconditionally());
    }
};

#endif //GWCLOUD_JOB_SERVER_DATABASEFIXTURE_H
