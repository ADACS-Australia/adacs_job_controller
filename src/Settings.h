//
// Created by lewis on 3/6/20.
//

#ifndef GWCLOUD_JOB_SERVER_SETTINGS_H
#define GWCLOUD_JOB_SERVER_SETTINGS_H

#include <cstdint>
#include <string>

// NOLINTNEXTLINE(bugprone-easily-swappable-parameters)
inline auto GET_ENV(const std::string &variable, const std::string &_default) -> std::string {
    // NOLINTNEXTLINE(clang-analyzer-cplusplus.StringChecker,concurrency-mt-unsafe)
    return std::getenv(variable.c_str()) != nullptr ? std::string(std::getenv(variable.c_str())) : _default;
}

#define DATABASE_USER               GET_ENV("DATABASE_USER", "jobserver")
#define DATABASE_PASSWORD           GET_ENV("DATABASE_PASSWORD", "jobserver")
#define DATABASE_SCHEMA             GET_ENV("DATABASE_SCHEMA", "jobserver")
#define DATABASE_HOST               GET_ENV("DATABASE_HOST", "localhost")
#define DATABASE_PORT               std::stoi(GET_ENV("DATABASE_PORT", "3306"))

#define MAX_FILE_BUFFER_SIZE        std::stoi(GET_ENV("MAX_FILE_BUFFER_SIZE", std::to_string(1024*1024*50)))
#define MIN_FILE_BUFFER_SIZE        std::stoi(GET_ENV("MIN_FILE_BUFFER_SIZE", std::to_string(1024*1024*10)))

#define FILE_DOWNLOAD_EXPIRY_TIME   std::stoi(GET_ENV("FILE_DOWNLOAD_EXPIRY_TIME", std::to_string(60*60*24)))

constexpr const char* CLUSTER_CONFIG_ENV_VARIABLE = "CLUSTER_CONFIG";
constexpr const char* ACCESS_SECRET_ENV_VARIABLE = "ACCESS_SECRET_CONFIG";

#ifndef BUILD_TESTS
    constexpr const char* BUNDLE_HTTP_PORT = ":80";
    constexpr const char* BUNDLE_HTTPS_PORT = ":443";
#else
    constexpr const char* BUNDLE_HTTP_PORT = ":23456";
    constexpr const char* BUNDLE_HTTPS_PORT = ":23457";

    #undef MAX_FILE_BUFFER_SIZE
    #undef MIN_FILE_BUFFER_SIZE
    #define MAX_FILE_BUFFER_SIZE (1024ULL*1024ULL*4ULL)
    #define MIN_FILE_BUFFER_SIZE (1024ULL*1024ULL*1ULL)
#endif

const uint64_t MESSAGE_INITIAL_VECTOR_SIZE = (1024ULL*64ULL);

const uint32_t CLUSTER_RESEND_MESSAGE_INTERVAL_SECONDS = 60;
const uint32_t CLUSTER_RECENT_STATE_JOB_IGNORE_SECONDS = 60;

const uint32_t CLUSTER_MANAGER_CLUSTER_RECONNECT_SECONDS = 60;
const uint32_t CLUSTER_MANAGER_TOKEN_EXPIRY_SECONDS = 60;

const uint32_t CLIENT_TIMEOUT_SECONDS = 30;

const uint16_t HTTP_PORT = 8000;
const uint32_t HTTP_WORKER_POOL_SIZE = 32;

const uint16_t WEBSOCKET_PORT = 8001;
const uint32_t WEBSOCKET_WORKER_POOL_SIZE = 32;

const uint32_t QUEUE_SOURCE_PRUNE_SECONDS = 60;

#endif //GWCLOUD_JOB_SERVER_SETTINGS_H
