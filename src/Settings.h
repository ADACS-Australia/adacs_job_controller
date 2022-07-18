//
// Created by lewis on 3/6/20.
//

#ifndef GWCLOUD_JOB_SERVER_SETTINGS_H
#define GWCLOUD_JOB_SERVER_SETTINGS_H

// NOLINTNEXTLINE(clang-analyzer-cplusplus.StringChecker,concurrency-mt-unsafe)
#define GET_ENV(x, y) (std::getenv(x) != nullptr ? std::string(std::getenv(x)) : y)

#define DATABASE_USER               GET_ENV("DATABASE_USER", "jobserver")
#define DATABASE_PASSWORD           GET_ENV("DATABASE_PASSWORD", "jobserver")
#define DATABASE_SCHEMA             GET_ENV("DATABASE_SCHEMA", "jobserver")
#define DATABASE_HOST               GET_ENV("DATABASE_HOST", "localhost")
#define DATABASE_PORT               std::stoi(GET_ENV("DATABASE_PORT", "3306"))

#define MAX_FILE_BUFFER_SIZE        std::stoi(GET_ENV("MAX_FILE_BUFFER_SIZE", std::to_string(1024*1024*50)))
#define MIN_FILE_BUFFER_SIZE        std::stoi(GET_ENV("MIN_FILE_BUFFER_SIZE", std::to_string(1024*1024*10)))

#define FILE_DOWNLOAD_EXPIRY_TIME   std::stoi(GET_ENV("FILE_DOWNLOAD_EXPIRY_TIME", std::to_string(60*60*24)))

#define CLUSTER_CONFIG_ENV_VARIABLE "CLUSTER_CONFIG"
#define ACCESS_SECRET_ENV_VARIABLE  "ACCESS_SECRET_CONFIG"

#ifndef BUILD_TESTS
    #define BUNDLE_HTTP_PORT ":80"
    #define BUNDLE_HTTPS_PORT ":443"
#else
    #define BUNDLE_HTTP_PORT ":23456"
    #define BUNDLE_HTTPS_PORT ":23457"

    #undef MAX_FILE_BUFFER_SIZE
    #undef MIN_FILE_BUFFER_SIZE
    #define MAX_FILE_BUFFER_SIZE 1024ULL*1024ULL*4ULL
    #define MIN_FILE_BUFFER_SIZE 1024ULL*1024ULL*1ULL
#endif

#define MESSAGE_INITIAL_VECTOR_SIZE 1024ULL * 64ULL

#define CLUSTER_RESEND_MESSAGE_INTERVAL_SECONDS 60
#define CLUSTER_RECENT_STATE_JOB_IGNORE_SECONDS 60

#define CLUSTER_MANAGER_CLUSTER_RECONNECT_SECONDS 60
#define CLUSTER_MANAGER_TOKEN_EXPIRY_SECONDS 60

#endif //GWCLOUD_JOB_SERVER_SETTINGS_H
