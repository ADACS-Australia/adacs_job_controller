//
// Created by lewis on 2/27/20.
//

#ifndef GWCLOUD_JOB_SERVER_CLUSTER_H
#define GWCLOUD_JOB_SERVER_CLUSTER_H


#include <string>
#include <boost/uuid/uuid.hpp>
#include "../WebSocket/WebSocketServer.h"
#include "../Lib/Messaging/Message.h"
#include <vector>
#include <boost/concept_check.hpp>

#define FOLLY_NO_CONFIG
#define FOLLY_HAVE_MEMRCHR true
#define FOLLY_HAVE_LIBGFLAGS true
#include "../Lib/folly/folly/concurrency/UnboundedQueue.h"
#include "../Lib/folly/folly/concurrency/ConcurrentHashMap.h"

class ClusterManager;
class MessageScheduler;

class Cluster {
public:
    Cluster(std::string name, ClusterManager* pClusterManager);

    void connect(const std::string& token);

    std::string getName() { return name; }

    void setConnection(WsServer::Connection *pConnection);

    void queueMessage(std::string source, std::vector<uint8_t>* data, Message::Priority priority);

    void handleMessage(Message &message);

    bool isOnline();

private:
    void run();

    std::string name;
    WsServer::Connection* pConnection = nullptr;
    ClusterManager* pClusterManager = nullptr;

    mutable std::shared_mutex mutex_;
    mutable std::mutex dataCVMutex;
    bool dataReady{};
    std::condition_variable dataCV;
    std::vector<folly::ConcurrentHashMap<std::string, folly::UMPSCQueue<std::vector<uint8_t>*, false, 8>*>> queue;

    // Threads
    std::thread schedulerThread;
    std::thread pruneThread;
    std::thread resendThread;

    void pruneSources();
    void resendMessages();

    bool doesHigherPriorityDataExist(uint64_t maxPriority);

    void updateJob(Message &message);

    void checkUnsubmittedJobs();
};


#endif //GWCLOUD_JOB_SERVER_CLUSTER_H
