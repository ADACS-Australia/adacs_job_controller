//
// Interface for Cluster functionality
// This breaks circular dependencies by providing a pure interface
//

#ifndef GWCLOUD_JOB_SERVER_I_CLUSTER_H
#define GWCLOUD_JOB_SERVER_I_CLUSTER_H

#include <memory>
#include <string>
#include <vector>
#include <cstdint>
import Message;

// Forward declarations to avoid circular dependencies
struct sClusterDetails;

// Interface for cluster operations
class ICluster {
public:
    virtual ~ICluster() = default;
    
    // Basic cluster information
    virtual auto getName() const -> std::string = 0;
    virtual auto getClusterDetails() const -> std::shared_ptr<sClusterDetails> = 0;
    virtual auto isOnline() const -> bool = 0;
    
    // Message handling
    virtual void handleMessage(Message& message) = 0;
    virtual void sendMessage(Message& message) = 0;
    virtual void queueMessage(const std::string& source, const std::shared_ptr<std::vector<uint8_t>>& data, uint32_t priority) = 0;
    
    // Connection management
    virtual void setConnection(const std::shared_ptr<void>& connection) = 0; // void* to avoid WebSocket dependency
    virtual void close(bool force = false) = 0;
    
    // Lifecycle
    virtual void stop() = 0;
    
    // Role management
    virtual auto getRoleString() const -> std::string = 0;
    virtual auto getRole() const -> int = 0; // Using int to avoid enum dependency
    
    // Dependency injection
    virtual void setClusterManager(const std::shared_ptr<void>& clusterManager) = 0;
};

#endif //GWCLOUD_JOB_SERVER_I_CLUSTER_H
