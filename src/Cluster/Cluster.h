//
// Created by lewis on 2/27/20.
//

#ifndef GWCLOUD_JOB_SERVER_CLUSTER_H
#define GWCLOUD_JOB_SERVER_CLUSTER_H

#include "../Lib/GeneralUtils.h"
import Message;
#include "../Lib/FileTypes.h"
#include "../Lib/GlobalState.h"
#include "../WebSocket/WebSocketServer.h"
#include "../Interfaces/ICluster.h"
#include <boost/concept_check.hpp>
#include <boost/uuid/uuid.hpp>
#include <folly/concurrency/ConcurrentHashMap.h>
#include <folly/concurrency/UnboundedQueue.h>
#include <nlohmann/json.hpp>
#include <shared_mutex>
#include <string>
#include <vector>

class FileDownload;

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



class Cluster : public std::enable_shared_from_this<Cluster>, public ICluster {
public:
    explicit Cluster(std::shared_ptr<sClusterDetails> details);
    virtual ~Cluster();
    Cluster(Cluster const&) = delete;
    auto operator =(Cluster const&) -> Cluster& = delete;
    Cluster(Cluster&&) = delete;
    auto operator=(Cluster&&) -> Cluster& = delete;

    void stop();

    // ICluster interface implementation
    auto getName() const -> std::string override { return pClusterDetails->getName(); }

    auto getClusterDetails() const -> std::shared_ptr<sClusterDetails> override { return pClusterDetails; }

    void setConnection(const std::shared_ptr<void>& connection) override;

    virtual void handleMessage(Message &message) override;

    virtual void sendMessage(Message &message) override;

    auto isOnline() const -> bool override;

    // virtual here so that we can override this function for testing
    virtual void queueMessage(const std::string& source, const std::shared_ptr<std::vector<uint8_t>>& data, uint32_t priority) override;

    enum eRole {
        master,
        fileDownload
    };

    // ICluster interface implementation
    auto getRoleString() const -> std::string override {
        return roleString;
    }

    auto getRole() const -> int override {
        return static_cast<int>(role);
    }

    void close(bool bForce = false);

    // ICluster interface implementation
    void setClusterManager(const std::shared_ptr<void>& clusterManager) override;

protected:
    eRole role = eRole::master;
    std::string roleString = "master";

private:
    std::shared_ptr<sClusterDetails> pClusterDetails = nullptr;
    std::shared_ptr<WsServer::Connection> pConnection = nullptr;
    std::shared_ptr<void> pClusterManager = nullptr; // void* to avoid circular dependency

    mutable std::mutex connectionMutex;
    mutable std::shared_mutex mutex_;
    mutable std::mutex dataCVMutex;
    bool dataReady{};
    std::condition_variable dataCV;
    std::vector<folly::ConcurrentHashMap<std::string, std::shared_ptr<folly::UMPSCQueue<std::shared_ptr<std::vector<uint8_t>>, false>>>> queue;

    bool bRunning = true;
    InterruptableTimer interruptableResendTimer;
    InterruptableTimer interruptablePruneTimer;

    // Threads
    std::jthread schedulerThread;
    std::jthread pruneThread;
    std::jthread resendThread;

    void run();
    void pruneSources();
    void resendMessages();

    auto doesHigherPriorityDataExist(uint64_t maxPriority) -> bool;

    void updateJob(Message &message);

    void checkUnsubmittedJobs();
    void checkCancellingJobs();
    void checkDeletingJobs();

    static void handleFileList(Message &message);
    static void handleFileListError(Message &message);

// Testing
    EXPOSE_PROPERTY_FOR_TESTING(pConnection);
    EXPOSE_PROPERTY_FOR_TESTING(pClusterDetails);
    EXPOSE_PROPERTY_FOR_TESTING(bRunning);
    EXPOSE_PROPERTY_FOR_TESTING_READONLY(queue);
    EXPOSE_PROPERTY_FOR_TESTING_READONLY(dataReady);
    EXPOSE_PROPERTY_FOR_TESTING_READONLY(dataCV);
    EXPOSE_PROPERTY_FOR_TESTING_READONLY(interruptablePruneTimer);

    EXPOSE_FUNCTION_FOR_TESTING(pruneSources);
    EXPOSE_FUNCTION_FOR_TESTING(run);
    EXPOSE_FUNCTION_FOR_TESTING(checkUnsubmittedJobs);
    EXPOSE_FUNCTION_FOR_TESTING(checkCancellingJobs);
    EXPOSE_FUNCTION_FOR_TESTING(checkDeletingJobs);

    EXPOSE_FUNCTION_FOR_TESTING_ONE_PARAM(handleMessage, Message&);
    EXPOSE_FUNCTION_FOR_TESTING_ONE_PARAM(doesHigherPriorityDataExist, uint64_t);
};


#endif //GWCLOUD_JOB_SERVER_CLUSTER_H
