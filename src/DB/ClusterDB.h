//
// Created by lewis on 10/8/22.
//

#ifndef GWCLOUD_JOB_SERVER_CLUSTERDB_H
#define GWCLOUD_JOB_SERVER_CLUSTERDB_H

#include "../Lib/Messaging/Message.h"
#include <memory>


class ClusterDB {
public:
    static auto maybeHandleClusterDBMessage(Message& message, const std::shared_ptr<Cluster>& pCluster) -> bool;

    static void getClusterJobByJobId(Message &message, const std::shared_ptr<Cluster> &pCluster);

    static void getClusterJobById(Message &message, const std::shared_ptr<Cluster> &pCluster);

    static void getRunningClusterJobs(Message &message, const std::shared_ptr<Cluster> &pCluster);

    static void deleteClusterJob(Message &message, const std::shared_ptr<Cluster> &pCluster);

    static void saveClusterJob(Message &message, const std::shared_ptr<Cluster> &pCluster);

    static void getClusterJobStatusByJobIdAndWhat(Message &message, const std::shared_ptr<Cluster> &pCluster);

    static void getClusterJobStatusByJobId(Message &message, const std::shared_ptr<Cluster> &pCluster);

    static void deleteClusterJobStatusByIdList(Message &message, const std::shared_ptr<Cluster> &pCluster);

    static void saveClusterJobStatus(Message &message, const std::shared_ptr<Cluster> &pCluster);

    static auto prepareResult(Message &message, const std::shared_ptr<Cluster> &pCluster) -> Message;
};

#endif //GWCLOUD_JOB_SERVER_CLUSTERDB_H
