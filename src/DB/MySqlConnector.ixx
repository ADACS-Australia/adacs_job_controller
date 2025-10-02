//
// Created by lewis on 5/3/20.
//

module;
#include <sqlpp11/mysql/mysql.h>

export module MySqlConnector;

import settings;

namespace mysql = sqlpp::mysql;

export class MySqlConnector
{
public:
    MySqlConnector()
    {
        auto config      = std::make_shared<mysql::connection_config>();
        config->user     = DATABASE_USER;
        config->database = DATABASE_SCHEMA;
        config->password = DATABASE_PASSWORD;
        config->host     = DATABASE_HOST;
        config->port     = DATABASE_PORT;

        // Note: auto_reconnect was removed in SQLPP11 0.65+ as MySQL 8.0.34 deprecated MYSQL_OPT_RECONNECT
#ifdef NDEBUG
        config->debug = false;
#else
        config->debug = true;
#endif
        database = std::make_shared<mysql::connection>(config);
    }

    virtual ~MySqlConnector()                                = default;
    MySqlConnector(MySqlConnector const&)                    = delete;
    auto operator=(MySqlConnector const&) -> MySqlConnector& = delete;
    MySqlConnector(MySqlConnector&&)                         = delete;
    auto operator=(MySqlConnector&&) -> MySqlConnector&      = delete;

    auto operator->() const -> std::shared_ptr<mysql::connection>
    {
        return database;
    }

    [[nodiscard]] auto getDb() const -> std::shared_ptr<mysql::connection>
    {
        return database;
    }

private:
    std::shared_ptr<mysql::connection> database;
};
