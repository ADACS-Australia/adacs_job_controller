//
// Interface for ClusterManager functionality
// This breaks circular dependencies by providing a pure interface
//

module;
#include <cstdint>
#include <memory>
#include <string>

#include <boost/system/error_code.hpp>

#include "../third_party/Simple-WebSocket-Server/server_ws.hpp"

export module IClusterManager;

import ICluster;

// Define the WsServer alias
using WsServer = SimpleWeb::SocketServer<SimpleWeb::WS>;

// Interface for cluster management operations
export class IClusterManager
{
public:
    virtual ~IClusterManager() = default;

    // Prevent copying and moving
    IClusterManager(const IClusterManager&)            = delete;
    IClusterManager& operator=(const IClusterManager&) = delete;
    IClusterManager(IClusterManager&&)                 = delete;
    IClusterManager& operator=(IClusterManager&&)      = delete;

    // Cluster lifecycle
    virtual void start()                                                                                          = 0;
    virtual auto handleNewConnection(const std::shared_ptr<WsServer::Connection>& connection,
                                     const std::string& uuid) -> std::shared_ptr<ICluster>                        = 0;
    virtual void removeConnection(const std::shared_ptr<WsServer::Connection>& connection, bool close, bool lock) = 0;

    // Cluster queries
    virtual auto getCluster(const std::shared_ptr<WsServer::Connection>& connection) -> std::shared_ptr<ICluster> = 0;
    virtual auto getCluster(const std::string& cluster) -> std::shared_ptr<ICluster>                              = 0;
    virtual auto isClusterOnline(const std::shared_ptr<ICluster>& cluster) -> bool                                = 0;

    // Error reporting
    virtual void reportWebsocketError(const std::shared_ptr<ICluster>& cluster,
                                      const boost::system::error_code& errorCode) = 0;

    // File download management
    virtual auto createFileDownload(const std::shared_ptr<ICluster>& cluster, const std::string& uuid) -> std::shared_ptr<ICluster> = 0;
    
    // File upload management  
    virtual auto createFileUpload(const std::shared_ptr<ICluster>& cluster, const std::string& uuid) -> std::shared_ptr<ICluster> = 0;
    
    // Connection management
    virtual void handlePong(const std::shared_ptr<WsServer::Connection>& connection) = 0;

protected:
    IClusterManager() = default;
};
