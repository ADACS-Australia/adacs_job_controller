//
// Created by lewis on 6/7/21.
//

#ifndef GWCLOUD_JOB_SERVER_HANDLEFILELIST_H
#define GWCLOUD_JOB_SERVER_HANDLEFILELIST_H

#include "../../Cluster/ClusterManager.h"
#include "../HttpServer.h"

void handleFileList(
        const std::shared_ptr<ClusterManager>& clusterManager, uint32_t jobId, bool bRecursive, const std::string &filePath,
        const std::string &appName, const std::vector<std::string> &applications,
        const std::shared_ptr<HttpServerImpl::Response> &response
);

void handleFileList(
        const std::shared_ptr<Cluster> &cluster, const std::string &sBundle, uint32_t jobId, bool bRecursive, const std::string &filePath,
        const std::shared_ptr<HttpServerImpl::Response> &response
);

auto filterFiles(const std::vector<sFile>& files, const std::string& filePath, bool bRecursive) -> std::vector<sFile>;


#endif //GWCLOUD_JOB_SERVER_HANDLEFILELIST_H
