module;
#include <cstdint>
#include <cstdlib>
#include <string>
export module settings;

// NOLINTNEXTLINE(bugprone-easily-swappable-parameters)
export inline auto GET_ENV(const std::string& variable, const std::string& _default) -> std::string
{
    // NOLINTNEXTLINE(clang-analyzer-cplusplus.StringChecker,concurrency-mt-unsafe)
    return std::getenv(variable.c_str()) != nullptr ? std::string(std::getenv(variable.c_str())) : _default;
}

export const std::string DATABASE_USER     = GET_ENV("DATABASE_USER", "jobserver");
export const std::string DATABASE_PASSWORD = GET_ENV("DATABASE_PASSWORD", "jobserver");
export const std::string DATABASE_SCHEMA   = GET_ENV("DATABASE_SCHEMA", "jobserver");
export const std::string DATABASE_HOST     = GET_ENV("DATABASE_HOST", "localhost");
export const int DATABASE_PORT             = std::stoi(GET_ENV("DATABASE_PORT", "3306"));

export const int FILE_DOWNLOAD_EXPIRY_TIME =
    std::stoi(GET_ENV("FILE_DOWNLOAD_EXPIRY_TIME", std::to_string(60 * 60 * 24)));

export constexpr const char* CLUSTER_CONFIG_ENV_VARIABLE = "CLUSTER_CONFIG";
export constexpr const char* ACCESS_SECRET_ENV_VARIABLE  = "ACCESS_SECRET_CONFIG";

#ifndef BUILD_TESTS
export constexpr const char* BUNDLE_HTTP_PORT  = ":80";
export constexpr const char* BUNDLE_HTTPS_PORT = ":443";

export const int MAX_FILE_BUFFER_SIZE = std::stoi(GET_ENV("MAX_FILE_BUFFER_SIZE", std::to_string(1024 * 1024 * 50)));
export const int MIN_FILE_BUFFER_SIZE = std::stoi(GET_ENV("MIN_FILE_BUFFER_SIZE", std::to_string(1024 * 1024 * 10)));

export constexpr const uint32_t QUEUE_SOURCE_PRUNE_MILLISECONDS        = 60000;
export constexpr uint32_t CLUSTER_RESEND_MESSAGE_INTERVAL_MILLISECONDS = 60000;
export constexpr uint32_t CLIENT_TIMEOUT_SECONDS                       = 30;
#else
export constexpr const char* BUNDLE_HTTP_PORT  = ":23456";
export constexpr const char* BUNDLE_HTTPS_PORT = ":23457";

// Override buffer sizes for tests
export const int MAX_FILE_BUFFER_SIZE = 1024 * 1024 * 4;
export const int MIN_FILE_BUFFER_SIZE = 1024 * 1024 * 1;

export constexpr uint32_t QUEUE_SOURCE_PRUNE_MILLISECONDS              = 500;
export constexpr uint32_t CLUSTER_RESEND_MESSAGE_INTERVAL_MILLISECONDS = 500;
export constexpr uint32_t CLIENT_TIMEOUT_SECONDS                       = 1;
#endif

export const uint64_t MESSAGE_INITIAL_VECTOR_SIZE = (1024ULL * 64ULL);

export const uint32_t CLUSTER_RECENT_STATE_JOB_IGNORE_SECONDS = 60;

export const uint32_t CLUSTER_MANAGER_CLUSTER_RECONNECT_SECONDS = 60;
export const uint32_t CLUSTER_MANAGER_PING_INTERVAL_SECONDS     = 10;
export const uint32_t CLUSTER_MANAGER_TOKEN_EXPIRY_SECONDS      = 60;

export const uint16_t HTTP_PORT                    = 8000;
export const uint32_t HTTP_WORKER_POOL_SIZE        = 1024;
export const uint32_t HTTP_CONTENT_TIMEOUT_SECONDS = (60ULL * 60ULL * 24ULL);

export const uint16_t WEBSOCKET_PORT             = 8001;
export const uint32_t WEBSOCKET_WORKER_POOL_SIZE = 1024;
