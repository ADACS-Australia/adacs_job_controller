//
// Created by lewis on 2/27/20.
//

#ifndef GWCLOUD_JOB_SERVER_CLUSTERMANAGER_H
#define GWCLOUD_JOB_SERVER_CLUSTERMANAGER_H

#include "../Lib/GeneralUtils.h"
#include "../WebSocket/WebSocketServer.h"
#include "Cluster.h"
#include "FileDownload.h"
#include "../Interfaces/IClusterManager.h"
#include <thread>

class ClusterManager : public IClusterManager {
public:
    ClusterManager();

    // IClusterManager interface implementation
    void start() override;
    auto handleNewConnection(const std::shared_ptr<WsServer::Connection>& connection, const std::string& uuid) -> std::shared_ptr<ICluster> override;
    void removeConnection(const std::shared_ptr<WsServer::Connection>& connection, bool close = true, bool lock = true) override;
    void handlePong(const std::shared_ptr<WsServer::Connection>& connection) override;
    auto getCluster(const std::shared_ptr<WsServer::Connection>& connection) -> std::shared_ptr<ICluster> override;
    auto getCluster(const std::string& cluster) -> std::shared_ptr<ICluster> override;
    auto isClusterOnline(const std::shared_ptr<ICluster>& cluster) -> bool override;
    void reportWebsocketError(const std::shared_ptr<ICluster>& cluster, const boost::system::error_code &errorCode) override;
    auto createFileDownload(const std::shared_ptr<ICluster>& cluster, const std::string& uuid) -> std::shared_ptr<FileDownload> override;

    struct sPingPongTimes {
        std::chrono::time_point<std::chrono::system_clock> pingTimestamp;
        std::chrono::time_point<std::chrono::system_clock> pongTimestamp;
    };
private:
    [[noreturn]] void run();
    [[noreturn]] void runPings();

    std::vector<std::shared_ptr<Cluster>> vClusters;
    std::map<std::shared_ptr<WsServer::Connection>, std::shared_ptr<Cluster>> mConnectedClusters;
    std::map<std::shared_ptr<WsServer::Connection>, std::shared_ptr<FileDownload>> mConnectedFileDownloads;

    std::map<std::shared_ptr<WsServer::Connection>, sPingPongTimes> mClusterPings;

    void reconnectClusters();
    static void connectCluster(const std::shared_ptr<Cluster>& cluster, const std::string &token);
    void checkPings();

    folly::ConcurrentHashMap<std::string, std::shared_ptr<FileDownload>> fileDownloadMap;

// Testing
EXPOSE_PROPERTY_FOR_TESTING(vClusters);
EXPOSE_PROPERTY_FOR_TESTING(mConnectedClusters);
EXPOSE_PROPERTY_FOR_TESTING(mConnectedFileDownloads);
EXPOSE_PROPERTY_FOR_TESTING(mClusterPings);
EXPOSE_PROPERTY_FOR_TESTING_READONLY(fileDownloadMap);
EXPOSE_FUNCTION_FOR_TESTING(reconnectClusters);
EXPOSE_FUNCTION_FOR_TESTING(checkPings);
};


#endif //GWCLOUD_JOB_SERVER_CLUSTERMANAGER_H
