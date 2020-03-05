//
// Created by lewis on 5/3/20.
//

#ifndef GWCLOUD_JOB_SERVER_MYSQLCONNECTOR_H
#define GWCLOUD_JOB_SERVER_MYSQLCONNECTOR_H

#include <sqlpp11/mysql/connection_config.h>
#include <sqlpp11/mysql/connection.h>
#include <sqlpp11/sqlpp11.h>

namespace mysql = sqlpp::mysql;

class MySqlConnector {
public:
    MySqlConnector() {
        auto config = std::make_shared<mysql::connection_config>();
        config->user = "jobserver";
        config->database = "jobserver";
        config->password = "jobserver";
        config->debug = true;
        db = new mysql::connection(config);
    }

    ~MySqlConnector() {
        delete db;
    }

    mysql::connection *operator->() const
    { return db; }

private:
    mysql::connection* db;
};

#endif //GWCLOUD_JOB_SERVER_MYSQLCONNECTOR_H
