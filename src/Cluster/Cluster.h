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
#include <nlohmann/json.hpp>
#include <shared_mutex>
#include "../Lib/GeneralUtils.h"

#define FOLLY_NO_CONFIG
#define FOLLY_HAVE_MEMRCHR true
#define FOLLY_HAVE_LIBGFLAGS true

#include "../Lib/folly/folly/concurrency/UnboundedQueue.h"
#include "../Lib/folly/folly/concurrency/ConcurrentHashMap.h"

class ClusterManager;

struct sFileDownload {
    folly::USPSCQueue<std::vector<uint8_t> *, false, 8> queue;
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

struct sClusterDetails {
    explicit inline sClusterDetails(nlohmann::json cluster) {
        name = cluster["name"];
        host = cluster["host"];
        username = cluster["username"];
        path = cluster["path"];
        key = cluster["key"];
    }

    auto getName() { return name; };

    auto getSshHost() { return host; };

    auto getSshUsername() { return username; };

    auto getSshPath() { return path; };

    auto getSshKey() { return key; };

private:
    std::string name;
    std::string host;
    std::string username;
    std::string path;
    std::string key;
};

extern folly::ConcurrentHashMap<std::string, sFileDownload *> fileDownloadMap;
extern folly::ConcurrentHashMap<std::string, sFileList *> fileListMap;
extern std::mutex fileDownloadMapDeletionLockMutex;

class Cluster {
public:
    Cluster(sClusterDetails *details, ClusterManager *pClusterManager);

    auto getName() { return pClusterDetails->getName(); }

    auto getClusterDetails() { return pClusterDetails; }

    void setConnection(WsServer::Connection *pCon);

    void handleMessage(Message &message);

    bool isOnline();

    // virtual here so that we can override this function for testing
    virtual void queueMessage(std::string source, std::vector<uint8_t> *data, Message::Priority priority);

private:
#ifndef BUILD_TESTS
    [[noreturn]] void run();
#else
    void run();
#endif

    sClusterDetails *pClusterDetails = nullptr;
    WsServer::Connection *pConnection = nullptr;
    ClusterManager *pClusterManager = nullptr;

    mutable std::shared_mutex mutex_;
    mutable std::mutex dataCVMutex;
    bool dataReady{};
    std::condition_variable dataCV;
    std::vector<folly::ConcurrentHashMap<std::string, folly::UMPSCQueue<std::vector<uint8_t> *, false, 8> *>> queue;

    // Threads
    std::thread schedulerThread;
    std::thread pruneThread;
    std::thread resendThread;

#ifndef BUILD_TESTS
    [[noreturn]] void pruneSources();
#else
    void pruneSources();
#endif

#ifndef BUILD_TESTS
    [[noreturn]] void resendMessages();
#else
    void resendMessages();
#endif

    bool doesHigherPriorityDataExist(uint64_t maxPriority);

    void updateJob(Message &message);

    void checkUnsubmittedJobs();

    static void handleFileError(Message &message);

    static void handleFileDetails(Message &message);

    void handleFileChunk(Message &message);

    static void handleFileList(Message &message);

// Testing
    EXPOSE_PROPERTY_FOR_TESTING(pConnection);
    EXPOSE_PROPERTY_FOR_TESTING(pClusterDetails);
    EXPOSE_PROPERTY_FOR_TESTING(pClusterManager);
    EXPOSE_PROPERTY_FOR_TESTING_READONLY(queue);
    EXPOSE_PROPERTY_FOR_TESTING_READONLY(dataReady);
    EXPOSE_PROPERTY_FOR_TESTING_READONLY(dataCV);

    EXPOSE_FUNCTION_FOR_TESTING(pruneSources);
    EXPOSE_FUNCTION_FOR_TESTING(run);
    EXPOSE_FUNCTION_FOR_TESTING(resendMessages);

    EXPOSE_FUNCTION_FOR_TESTING_ONE_PARAM(handleMessage, Message&);
    EXPOSE_FUNCTION_FOR_TESTING_ONE_PARAM(doesHigherPriorityDataExist, uint64_t);
};


#endif //GWCLOUD_JOB_SERVER_CLUSTER_H
