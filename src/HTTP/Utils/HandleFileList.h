//
// Created by lewis on 6/7/21.
//

#ifndef GWCLOUD_JOB_SERVER_HANDLEFILELIST_H
#define GWCLOUD_JOB_SERVER_HANDLEFILELIST_H

#include "../HttpServer.h"
#include "../../Cluster/ClusterManager.h"

void handleFileList(
        std::shared_ptr<ClusterManager> clusterManager, uint32_t jobId, bool bRecursive, const std::string &filePath,
        const std::string &appName, const std::vector<std::string> &applications,
        HttpServerImpl::Response *response
);

void handleFileList(
        std::shared_ptr<Cluster> cluster, auto jobId, bool bRecursive, const std::string &filePath,
        const std::string &appName, const std::vector<std::string> &applications,
        HttpServerImpl::Response *response
);

std::vector<sFile> filterFiles(const std::vector<sFile>& files, const std::string& filePath, bool bRecursive);


#endif //GWCLOUD_JOB_SERVER_HANDLEFILELIST_H
