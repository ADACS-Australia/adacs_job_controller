//
// Interface for ClusterManager functionality
// This breaks circular dependencies by providing a pure interface
//

module;
#include <memory>
#include <string>
#include <cstdint>
#include <boost/system/error_code.hpp>
#include "../third_party/Simple-WebSocket-Server/server_ws.hpp"

export module IClusterManager;

import ICluster;

// Forward declarations to avoid circular dependencies
class FileDownload;

// Define the WsServer alias
using WsServer = SimpleWeb::SocketServer<SimpleWeb::WS>;

// Interface for cluster management operations
export class IClusterManager {
public:
    virtual ~IClusterManager() = default;
    
    // Cluster lifecycle
    virtual void start() = 0;
    virtual auto handleNewConnection(const std::shared_ptr<WsServer::Connection>& connection, const std::string& uuid) -> std::shared_ptr<ICluster> = 0;
    virtual void removeConnection(const std::shared_ptr<WsServer::Connection>& connection, bool close = true, bool lock = true) = 0;
    
    // Cluster queries
    virtual auto getCluster(const std::shared_ptr<WsServer::Connection>& connection) -> std::shared_ptr<ICluster> = 0;
    virtual auto getCluster(const std::string& cluster) -> std::shared_ptr<ICluster> = 0;
    virtual auto isClusterOnline(const std::shared_ptr<ICluster>& cluster) -> bool = 0;
    
    // Error reporting
    virtual void reportWebsocketError(const std::shared_ptr<ICluster>& cluster, const boost::system::error_code& errorCode) = 0;
    
    // File download management
    virtual auto createFileDownload(const std::shared_ptr<ICluster>& cluster, const std::string& uuid) -> std::shared_ptr<FileDownload> = 0;
    
    // Connection management
    virtual void handlePong(const std::shared_ptr<WsServer::Connection>& connection) = 0;
};
