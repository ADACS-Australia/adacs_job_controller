//
// Created by lewis on 6/7/21.
//

#ifndef GWCLOUD_JOB_SERVER_HANDLEFILELIST_H
#define GWCLOUD_JOB_SERVER_HANDLEFILELIST_H

#include "../../Interfaces/ICluster.h"
#include "../../Interfaces/IClusterManager.h"
#include "../../Lib/FileTypes.h"  // For sFile struct
#include "../HttpServer.h"

// Forward declarations
class ClusterManager;

void handleFileList(
        const std::shared_ptr<IClusterManager>& clusterManager, uint64_t jobId, bool bRecursive, const std::string &filePath,
        const std::string &appName, const std::vector<std::string> &applications,
        const std::shared_ptr<HttpServerImpl::Response> &response
);

void handleFileList(
        const std::shared_ptr<ICluster> &cluster, const std::string &sBundle, uint64_t jobId, bool bRecursive, const std::string &filePath,
        const std::shared_ptr<HttpServerImpl::Response> &response
);

auto filterFiles(const std::vector<sFile>& files, const std::string& filePath, bool bRecursive) -> std::vector<sFile>;


#endif //GWCLOUD_JOB_SERVER_HANDLEFILELIST_H
