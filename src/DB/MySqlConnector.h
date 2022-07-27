//
// Created by lewis on 5/3/20.
//

#ifndef GWCLOUD_JOB_SERVER_MYSQLCONNECTOR_H
#define GWCLOUD_JOB_SERVER_MYSQLCONNECTOR_H

#include "../Settings.h"
#include <sqlpp11/mysql/connection.h>
#include <sqlpp11/mysql/connection_config.h>
#include <sqlpp11/sqlpp11.h>

namespace mysql = sqlpp::mysql;

class MySqlConnector {
public:
    MySqlConnector() {
        auto config = std::make_shared<mysql::connection_config>();
        config->user = DATABASE_USER;
        config->database = DATABASE_SCHEMA;
        config->password = DATABASE_PASSWORD;
        config->host = DATABASE_HOST;
        config->port = DATABASE_PORT;
#ifdef NDEBUG
        config->debug = false;
#else
        config->debug = true;
#endif
        database = std::make_shared<mysql::connection>(config);
    }

    virtual ~MySqlConnector() = default;
    MySqlConnector(MySqlConnector const&) = delete;
    auto operator =(MySqlConnector const&) -> MySqlConnector& = delete;
    MySqlConnector(MySqlConnector&&) = delete;
    auto operator=(MySqlConnector&&) -> MySqlConnector& = delete;

    auto operator->() const -> std::shared_ptr<mysql::connection>
    { return database; }

    [[nodiscard]] auto getDb() const -> std::shared_ptr<mysql::connection>
    { return database; }

private:
    std::shared_ptr<mysql::connection> database;
};

#endif //GWCLOUD_JOB_SERVER_MYSQLCONNECTOR_H
