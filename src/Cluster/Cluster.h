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

struct sFileDownload {
    folly::USPSCQueue<std::vector<uint8_t>*, false, 8> queue;
    uint64_t fileSize = -1;
    bool error = false;
    std::string errorDetails;
    mutable std::mutex dataCVMutex;
    bool dataReady = false;
    std::condition_variable dataCV;
    bool receivedData = false;
    uint64_t receivedBytes = 0;
    uint64_t sentBytes = 0;
    bool clientPaused = false;
};

struct sFile {
    std::string fileName;
    uint64_t fileSize;
    uint32_t permissions;
    bool isDirectory;
};

struct sFileList {
    std::vector<sFile> files;
    bool error = false;
    std::string errorDetails;
    mutable std::mutex dataCVMutex;
    bool dataReady = false;
    std::condition_variable dataCV;
};

extern folly::ConcurrentHashMap<std::string, sFileDownload*> fileDownloadMap;
extern folly::ConcurrentHashMap<std::string, sFileList *> fileListMap;

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

    static void handleFileError(Message &message);

    static void handleFileDetails(Message &message);

    void handleFileChunk(Message &message);

    static void handleFileList(Message &message);
};


#endif //GWCLOUD_JOB_SERVER_CLUSTER_H
