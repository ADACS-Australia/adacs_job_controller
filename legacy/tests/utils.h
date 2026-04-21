#ifndef GWCLOUD_JOB_SERVER_UTILS_H
#define GWCLOUD_JOB_SERVER_UTILS_H

#include <filesystem>
#include <fstream>
#include <iomanip>
#include <iostream>
#include <limits>
#include <random>
#include <sstream>
#include <string>
#include <vector>

#include <client_http.hpp>
#include <client_ws.hpp>
#include <server_http.hpp>
#include <server_https.hpp>
#include <server_ws.hpp>

auto getLastToken() -> std::string;
auto randomInt(uint64_t start, uint64_t end) -> uint64_t;
auto generateRandomData(uint32_t count) -> std::shared_ptr<std::vector<uint8_t>>;
auto parseLine(char* line) -> size_t;
auto getCurrentMemoryUsage() -> size_t;

/**
 * RAII class for creating a temporary JSON file that is automatically cleaned up
 * Uses portable std::filesystem for temporary directory and random filenames
 */
class TemporaryJsonFile
{
public:
    explicit TemporaryJsonFile(const std::string& content)
    {
        // Generate a random filename in the temp directory
        filepath = generateTempFilePath();

        // Write the content to the file
        std::ofstream file(filepath);
        if (!file.is_open())
        {
            throw std::runtime_error("Failed to create temporary file: " + filepath.string());
        }
        file << content;
        file.close();
    }

    ~TemporaryJsonFile()
    {
        // Clean up the temporary file
        std::error_code ec;
        std::filesystem::remove(filepath, ec);
        // Ignore errors during cleanup (file might already be deleted)
    }

    // Delete copy constructor and assignment operator
    TemporaryJsonFile(const TemporaryJsonFile&)            = delete;
    TemporaryJsonFile& operator=(const TemporaryJsonFile&) = delete;

    // Allow move operations
    TemporaryJsonFile(TemporaryJsonFile&& other) noexcept : filepath(std::move(other.filepath))
    {
        other.filepath.clear();
    }

    TemporaryJsonFile& operator=(TemporaryJsonFile&& other) noexcept
    {
        if (this != &other)
        {
            // Clean up existing file
            std::error_code ec;
            std::filesystem::remove(filepath, ec);

            filepath = std::move(other.filepath);
            other.filepath.clear();
        }
        return *this;
    }

    [[nodiscard]] auto path() const -> std::string
    {
        return filepath.string();
    }

private:
    std::filesystem::path filepath;

    static auto generateTempFilePath() -> std::filesystem::path
    {
        // Get system temp directory
        auto tempDir = std::filesystem::temp_directory_path();

        // Generate random filename
        std::random_device rd;
        std::mt19937 gen(rd());
        std::uniform_int_distribution<uint64_t> dis(0, std::numeric_limits<uint64_t>::max());

        std::stringstream ss;
        ss << "test_config_" << std::hex << std::setfill('0') << std::setw(16) << dis(gen) << ".json";

        return tempDir / ss.str();
    }
};

/**
 * RAII helper for setting up cluster configuration test files
 * Automatically creates the temporary file and sets the environment variable
 */
class TempClusterConfig
{
public:
    explicit TempClusterConfig(const std::string& jsonContent) : tempFile(jsonContent)
    {
        setenv("CLUSTER_CONFIG_FILE", tempFile.path().c_str(), 1);
    }

    [[nodiscard]] auto path() const -> std::string
    {
        return tempFile.path();
    }

private:
    TemporaryJsonFile tempFile;
};

/**
 * RAII helper for setting up access secret configuration test files
 * Automatically creates the temporary file and sets the environment variable
 */
class TempAccessSecretConfig
{
public:
    explicit TempAccessSecretConfig(const std::string& jsonContent) : tempFile(jsonContent)
    {
        setenv("ACCESS_SECRET_CONFIG_FILE", tempFile.path().c_str(), 1);
    }

    [[nodiscard]] auto path() const -> std::string
    {
        return tempFile.path();
    }

private:
    TemporaryJsonFile tempFile;
};

using TestWsServer = SimpleWeb::SocketServer<SimpleWeb::WS>;
using TestWsClient = SimpleWeb::SocketClient<SimpleWeb::WS>;

using TestHttpServer  = SimpleWeb::Server<SimpleWeb::HTTP>;
using TestHttpsServer = SimpleWeb::Server<SimpleWeb::HTTPS>;

using TestHttpClient  = SimpleWeb::Client<SimpleWeb::HTTP>;
using TestHttpsClient = SimpleWeb::Client<SimpleWeb::HTTPS>;

#endif  // GWCLOUD_JOB_SERVER_UTILS_H