//
// Created by lewis on 2/27/20.
//

#ifndef GWCLOUD_JOB_SERVER_CLUSTER_H
#define GWCLOUD_JOB_SERVER_CLUSTER_H

#include "../Lib/GeneralUtils.h"
#include "../Lib/Messaging/Message.h"
#include <folly/concurrency/ConcurrentHashMap.h>
#include <folly/concurrency/UnboundedQueue.h>
#include "../WebSocket/WebSocketServer.h"
#include <boost/concept_check.hpp>
#include <boost/uuid/uuid.hpp>
#include <nlohmann/json.hpp>
#include <shared_mutex>
#include <string>
#include <vector>

class ClusterManager;

struct sFileDownload {
    folly::USPSCQueue<std::shared_ptr<std::vector<uint8_t>>, false> queue;
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

extern const std::shared_ptr<folly::ConcurrentHashMap<std::string, std::shared_ptr<sFileDownload>>> fileDownloadMap;
extern const std::shared_ptr<folly::ConcurrentHashMap<std::string, std::shared_ptr<sFileList>>> fileListMap;
// NOLINTBEGIN(cppcoreguidelines-avoid-non-const-global-variables)
extern std::mutex fileDownloadMapDeletionLockMutex;
extern std::mutex fileDownloadPauseResumeLockMutex;
extern std::mutex fileListMapDeletionLockMutex;
// NOLINTEND(cppcoreguidelines-avoid-non-const-global-variables)

class Cluster : public std::enable_shared_from_this<Cluster> {
public:
    explicit Cluster(std::shared_ptr<sClusterDetails> details);
    virtual ~Cluster() = default;
    Cluster(Cluster const&) = delete;
    auto operator =(Cluster const&) -> Cluster& = delete;
    Cluster(Cluster&&) = delete;
    auto operator=(Cluster&&) -> Cluster& = delete;

    auto getName() { return pClusterDetails->getName(); }

    auto getClusterDetails() { return pClusterDetails; }

    void setConnection(const std::shared_ptr<WsServer::Connection>& pCon);

    void handleMessage(Message &message);

    auto isOnline() -> bool;

    // virtual here so that we can override this function for testing
    virtual void queueMessage(std::string source, const std::shared_ptr<std::vector<uint8_t>>& data, Message::Priority priority);

private:
#ifndef BUILD_TESTS
    [[noreturn]] void run();
#else
    void run();
#endif

    std::shared_ptr<sClusterDetails> pClusterDetails = nullptr;
    std::shared_ptr<WsServer::Connection> pConnection = nullptr;

    mutable std::shared_mutex mutex_;
    mutable std::mutex dataCVMutex;
    bool dataReady{};
    std::condition_variable dataCV;
    std::vector<folly::ConcurrentHashMap<std::string, std::shared_ptr<folly::UMPSCQueue<std::shared_ptr<std::vector<uint8_t>>, false>>>> queue;

    // Threads
    std::thread schedulerThread;
    std::thread pruneThread;
    std::thread resendThread;

#ifndef BUILD_TESTS
    [[noreturn]] void pruneSources();
#else
    void pruneSources();
#endif

    [[noreturn]] void resendMessages();

    auto doesHigherPriorityDataExist(uint64_t maxPriority) -> bool;

    void updateJob(Message &message);

    void checkUnsubmittedJobs();
    void checkCancellingJobs();
    void checkDeletingJobs();

    static void handleFileError(Message &message);

    static void handleFileDetails(Message &message);

    void handleFileChunk(Message &message);

    static void handleFileList(Message &message);
    static void handleFileListError(Message &message);

// Testing
    EXPOSE_PROPERTY_FOR_TESTING(pConnection);
    EXPOSE_PROPERTY_FOR_TESTING(pClusterDetails);
    EXPOSE_PROPERTY_FOR_TESTING_READONLY(queue);
    EXPOSE_PROPERTY_FOR_TESTING_READONLY(dataReady);
    EXPOSE_PROPERTY_FOR_TESTING_READONLY(dataCV);

    EXPOSE_FUNCTION_FOR_TESTING(pruneSources);
    EXPOSE_FUNCTION_FOR_TESTING(run);
    EXPOSE_FUNCTION_FOR_TESTING(checkUnsubmittedJobs);
    EXPOSE_FUNCTION_FOR_TESTING(checkCancellingJobs);
    EXPOSE_FUNCTION_FOR_TESTING(checkDeletingJobs);

    EXPOSE_FUNCTION_FOR_TESTING_ONE_PARAM(handleMessage, Message&);
    EXPOSE_FUNCTION_FOR_TESTING_ONE_PARAM(doesHigherPriorityDataExist, uint64_t);
};


#endif //GWCLOUD_JOB_SERVER_CLUSTER_H
