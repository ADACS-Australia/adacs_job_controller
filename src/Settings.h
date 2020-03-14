//
// Created by lewis on 3/6/20.
//

#ifndef GWCLOUD_JOB_SERVER_SETTINGS_H
#define GWCLOUD_JOB_SERVER_SETTINGS_H

#define GET_ENV(x, y) (std::getenv(x) != nullptr ? std::string(std::getenv(x)) : y)

#define DATABASE_USER               GET_ENV("DATABASE_USER", "jobserver")
#define DATABASE_PASSWORD           GET_ENV("DATABASE_PASSWORD", "jobserver")
#define DATABASE_SCHEMA             GET_ENV("DATABASE_SCHEMA", "jobserver")
#define DATABASE_HOST               GET_ENV("DATABASE_HOST", "localhost")
#define DATABASE_PORT               std::stoi(GET_ENV("DATABASE_PORT", "3306"))

#define JWT_SECRET                  GET_ENV("JWT_SECRET", "^zzul@u)rxayk67^%3kf^59!pw&-vfv0lnv6#6h)w6!eyjzz!g")

#define MAX_FILE_BUFFER_SIZE        std::stoi(GET_ENV("MAX_FILE_BUFFER_SIZE", std::to_string(1024*1024*50)))
#define MIN_FILE_BUFFER_SIZE        std::stoi(GET_ENV("MIN_FILE_BUFFER_SIZE", std::to_string(1024*1024*10)))
#endif //GWCLOUD_JOB_SERVER_SETTINGS_H
