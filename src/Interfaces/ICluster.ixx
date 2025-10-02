//
// Interface for Cluster functionality
// This breaks circular dependencies by providing a pure interface
//

module;
#include <cstdint>
#include <memory>
#include <string>
#include <vector>

#include <nlohmann/json.hpp>

export module ICluster;

import Message;

// sClusterDetails definition
export struct sClusterDetails
{
    explicit sClusterDetails(nlohmann::json cluster)
    {
        // NOLINTBEGIN(cppcoreguidelines-pro-bounds-avoid-unchecked-container-access)
        name     = cluster["name"];
        host     = cluster["host"];
        username = cluster["username"];
        path     = cluster["path"];
        key      = cluster["key"];
        // NOLINTEND(cppcoreguidelines-pro-bounds-avoid-unchecked-container-access)
    }

    auto getName()
    {
        return name;
    }

    auto getSshHost()
    {
        return host;
    }

    auto getSshUsername()
    {
        return username;
    }

    auto getSshPath()
    {
        return path;
    }

    auto getSshKey()
    {
        return key;
    }

private:
    std::string name;
    std::string host;
    std::string username;
    std::string path;
    std::string key;
};

// Cluster role enumeration
export enum class eRole : std::uint8_t
{
    master,
    fileDownload
};

// Interface for cluster operations
export class ICluster
{
public:
    virtual ~ICluster() = default;

    // Prevent copying and moving
    ICluster(const ICluster&)            = delete;
    ICluster& operator=(const ICluster&) = delete;
    ICluster(ICluster&&)                 = delete;
    ICluster& operator=(ICluster&&)      = delete;

protected:
    // Allow derived classes to construct
    ICluster() = default;

public:
    // Basic cluster information
    [[nodiscard]] virtual auto getName() const -> std::string                                = 0;
    [[nodiscard]] virtual auto getClusterDetails() const -> std::shared_ptr<sClusterDetails> = 0;
    [[nodiscard]] virtual auto isOnline() const -> bool                                      = 0;

    // Message handling
    virtual void handleMessage(Message& message)          = 0;
    virtual void sendMessage(Message& message)            = 0;
    virtual void queueMessage(const std::string& source,
                              const std::shared_ptr<std::vector<uint8_t>>& data,
                              Message::Priority priority) = 0;

    // Connection management
    virtual void setConnection(const std::shared_ptr<void>& connection) = 0;  // void* to avoid WebSocket dependency
    virtual void close(bool force)                                      = 0;

    // Lifecycle
    virtual void stop() = 0;

    // Role management
    [[nodiscard]] virtual auto getRoleString() const -> std::string = 0;
    [[nodiscard]] virtual auto getRole() const -> eRole             = 0;

    // Dependency injection
    virtual void setClusterManager(const std::shared_ptr<void>& clusterManager) = 0;
};
