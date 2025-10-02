//
// Created by lewis on 2/27/20.
//

module;
#include <iostream>
#include <shared_mutex>
#include <string>
#include <vector>

#include <boost/concept_check.hpp>
#include <boost/uuid/uuid.hpp>
#include <client_http.hpp>
#include <folly/Uri.h>
#include <nlohmann/json.hpp>
#include <sqlpp11/sqlpp11.h>

#include "../Lib/FileTypes.h"
#include "../Lib/FollyTypes.h"
#include "../Lib/TestingMacros.h"
#include "../Lib/shims/sqlpp_shim.h"

export module Cluster;

import IClusterManager;
import Message;
import job_status;
import settings;
import ClusterDB;
import jobserver_schema;
import MySqlConnector;
import ICluster;
import IApplication;
import HandleFileList;
import WebSocketServer;
import GeneralUtils;

export class Cluster : public std::enable_shared_from_this<Cluster>, public ICluster
{
public:
    explicit Cluster(std::shared_ptr<sClusterDetails> details, std::shared_ptr<IApplication> app);
    ~Cluster() override;
    Cluster(Cluster const&)                    = delete;
    auto operator=(Cluster const&) -> Cluster& = delete;
    Cluster(Cluster&&)                         = delete;
    auto operator=(Cluster&&) -> Cluster&      = delete;

    void stop() override;

    // ICluster interface implementation
    [[nodiscard]] auto getName() const -> std::string override
    {
        return pClusterDetails->getName();
    }

    [[nodiscard]] auto getClusterDetails() const -> std::shared_ptr<sClusterDetails> override
    {
        return pClusterDetails;
    }

    void setConnection(const std::shared_ptr<void>& connection) override;

    void handleMessage(Message& message) override;

    void sendMessage(Message& message) override;

    auto isOnline() const -> bool override;

    // virtual here so that we can override this function for testing
    void queueMessage(const std::string& source,
                      const std::shared_ptr<std::vector<uint8_t>>& data,
                      Message::Priority priority) override;

    // ICluster interface implementation
    [[nodiscard]] auto getRoleString() const -> std::string override
    {
        return roleString;
    }

    auto getRole() const -> eRole override
    {
        return role;
    }

    void close(bool bForce) override;

    // ICluster interface implementation
    void setClusterManager(const std::shared_ptr<void>& clusterManager) override;

protected:
    eRole role             = eRole::master;
    std::string roleString = "master";
    std::shared_ptr<IApplication> app;

private:
    // Non-virtual cleanup method to avoid virtual dispatch during destruction
    void cleanup();

    std::shared_ptr<sClusterDetails> pClusterDetails  = nullptr;
    std::shared_ptr<WsServer::Connection> pConnection = nullptr;
    std::shared_ptr<void> pClusterManager             = nullptr;  // void* to avoid circular dependency

    mutable std::mutex connectionMutex;
    mutable std::shared_mutex mutex;
    mutable std::mutex dataCVMutex;
    bool dataReady{};
    std::condition_variable dataCV;
    // Use std::map for priority queue
    std::map<Message::Priority,
             folly::ConcurrentHashMap<std::string,
                                      std::shared_ptr<folly::UMPSCQueue<std::shared_ptr<std::vector<uint8_t>>, false>>>>
        queue;

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

    auto doesHigherPriorityDataExist(Message::Priority maxPriority) -> bool;

    void updateJob(Message& message);

    void checkUnsubmittedJobs();
    void checkCancellingJobs();
    void checkDeletingJobs();

    void handleFileList(Message& message);
    void handleFileListError(Message& message);

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
    EXPOSE_FUNCTION_FOR_TESTING_ONE_PARAM(doesHigherPriorityDataExist, Message::Priority);
};

// Packet Queue is a:
//  list of priorities - doesn't need any sync because it never changes
//      -> map of sources - needs sync when adding/removing sources
//          -> vector of packets - make this a MPSC queue

// When the number of bytes in a packet of vectors exceeds some amount, a message should be sent that stops more
// packets from being sent, when the vector then falls under some threshold

// Track sources in the map, add them when required - delete them after some amount (1 minute?) of inactivity.

// Send sources round robin, starting from the highest priority


Cluster::Cluster(std::shared_ptr<sClusterDetails> details, std::shared_ptr<IApplication> app)
    : pClusterDetails(std::move(details)), app(std::move(app))
{
    std::cout << "Cluster startup for role " << roleString << '\n';

    // Initialize queue for all priorities
    queue[Message::Priority::Highest];
    queue[Message::Priority::Medium];
    queue[Message::Priority::Lowest];

    // Start the scheduler thread
    schedulerThread = std::jthread([this] {
        this->run();
    });

    // Start the prune thread
    pruneThread = std::jthread([this] {
        // NOLINTBEGIN(clang-analyzer-security.ArrayBound)
        this->pruneSources();
        // NOLINTEND(clang-analyzer-security.ArrayBound)
    });

    // Start the resend thread if this is a master cluster
    if (role == eRole::master)
    {
        resendThread = std::jthread([this] {
            this->resendMessages();
        });
    }
}

Cluster::~Cluster()
{
    // Call the non-virtual cleanup method directly to avoid virtual dispatch during destruction
    cleanup();
}

void Cluster::stop()
{
    cleanup();
}

void Cluster::cleanup()
{
    bRunning = false;
    interruptablePruneTimer.stop();
    interruptableResendTimer.stop();

    dataReady = true;
    dataCV.notify_one();

    if (schedulerThread.joinable())
    {
        schedulerThread.join();
    }

    if (pruneThread.joinable())
    {
        pruneThread.join();
    }

    if (role == eRole::master && resendThread.joinable())
    {
        resendThread.join();
    }
}

void Cluster::handleMessage(Message& message)
{
    // Check if the message can be handled by the cluster database
    if (ClusterDB::maybeHandleClusterDBMessage(message, shared_from_this()))
    {
        return;
    }

    auto msgId = message.getId();

    switch (msgId)
    {
        case UPDATE_JOB:
            this->updateJob(message);
            break;
        case FILE_LIST:
            handleFileList(message);
            break;
        case FILE_LIST_ERROR:
            handleFileListError(message);
            break;
        default:
            std::cout << "Got invalid message ID " << msgId << " from " << this->getName() << '\n';
    }
}

void Cluster::sendMessage(Message& message)
{
    // Cluster is responsible for sending messages
    queueMessage(message.getSource(), message.getData(), message.getPriority());
}

void Cluster::setClusterManager(const std::shared_ptr<void>& clusterManager)
{
    pClusterManager = clusterManager;
}

void Cluster::setConnection(const std::shared_ptr<void>& connection)
{
    // Cast the void* back to the concrete type
    auto pCon = std::static_pointer_cast<WsServer::Connection>(connection);

    // Protect this block against race conditions. It's possible for pConnection to be
    // set to null in multiple threads.
    const std::unique_lock<std::mutex> closeLock(connectionMutex);

    this->pConnection = pCon;

    if (pCon != nullptr && getRole() == eRole::master)
    {
        // See if there are any pending jobs that should be sent
        checkUnsubmittedJobs();

        // See if there are any cancelling jobs that should be sent
        checkCancellingJobs();

        // See if there are any deleting jobs that should be sent
        checkDeletingJobs();
    }
}

void Cluster::queueMessage(const std::string& source,
                           const std::shared_ptr<std::vector<uint8_t>>& pData,
                           Message::Priority priority)
{
    // Get a pointer to the relevant map using priority mapping
    auto* pMap = &queue[priority];

    // Lock the access mutex to check if the source exists in the map
    {
        const std::shared_lock<std::shared_mutex> lock(mutex);

        // Make sure that this source exists in the map
        auto sQueue = std::make_shared<folly::UMPSCQueue<std::shared_ptr<std::vector<uint8_t>>, false>>();

        // Make sure that the source is in the map
        pMap->try_emplace(source, sQueue);

        // Write the data in the queue
        // NOLINTNEXTLINE(cppcoreguidelines-pro-bounds-avoid-unchecked-container-access)
        (*pMap)[source]->enqueue(pData);

        // Trigger the new data event to start sending
        this->dataReady = true;
        dataCV.notify_one();
    }
}

auto Cluster::isOnline() const -> bool
{
    return pConnection != nullptr;
}

void Cluster::close(bool bForce)
{
    // Protect this block against race conditions. It's possible for this function to be called from multiple threads
    // which can lead to segfaults without synchronising.
    const std::unique_lock<std::mutex> closeLock(connectionMutex);

    // Terminate the websocket connection
    if (pConnection)
    {
        try
        {
            if (bForce)
            {
                pConnection->close();
            }
            else
            {
                // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
                pConnection->send_close(1000, "Closing connection.");
            }
        }
        catch (...)  // NOLINT(bugprone-empty-catch)
        {
            // It's possible that we try to ask the connection to close when it's already internally been closed. This
            // can lead to cases where std::runtime_error or other exceptions can be thrown. It's safe to ignore this
            // case.
        }
        pConnection = nullptr;
    }
}

void Cluster::run()
{
    // Iterate while running
    while (bRunning)
    {
        {
            std::unique_lock<std::mutex> lock(dataCVMutex);

            // Wait for data to be ready to send
            dataCV.wait(lock, [this] {
                return this->dataReady;
            });

            // Reset the condition
            this->dataReady = false;
        }

    reset:

        // Iterate over the priorities in order
        for (const auto& [priority, pMap] : queue)
        {
            // While there is still data for this priority, send it
            bool hadData = false;
            do  // NOLINT(cppcoreguidelines-avoid-do-while)
            {
                hadData = false;

                const std::shared_lock<std::shared_mutex> lock(mutex);
                // Iterate over the map
                for (auto iter = pMap.begin(); iter != pMap.end(); ++iter)
                {
                    // Check if the vector for this source is empty
                    if (!(*iter).second->empty())
                    {
                        // Pop the next item from the queue
                        auto data = (*iter).second->try_dequeue();

                        try
                        {
                            // data should never be null as we're checking for empty
                            if (data)
                            {
                                // Convert the message
                                auto outMessage = std::make_shared<WsServer::OutMessage>((*data)->size());
                                std::copy((*data)->begin(),
                                          (*data)->end(),
                                          std::ostream_iterator<uint8_t>(*outMessage));

                                // Protect this block against race conditions. It's possible for pConnection to be
                                // set to null in multiple threads.
                                const std::unique_lock<std::mutex> closeLock(connectionMutex);

                                // Send the message on the websocket
                                if (pConnection != nullptr)
                                {
                                    std::promise<void> sendPromise;
                                    pConnection->send(
                                        outMessage,
                                        [this, &sendPromise](const SimpleWeb::error_code& errorCode) {
                                            // Data has been sent, set the promise
                                            sendPromise.set_value();

                                            // Kill the connection only if the error was not indicating success
                                            if (!errorCode)
                                            {
                                                return;
                                            }

                                            // Terminate the connection forcefully
                                            close(true);

                                            // Report error through injected cluster manager
                                            if (pClusterManager)
                                            {
                                                auto clusterManager =
                                                    std::static_pointer_cast<IClusterManager>(pClusterManager);
                                                clusterManager->reportWebsocketError(shared_from_this(), errorCode);
                                            }
                                        },
                                        // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
                                        130);

                                    // Wait for the data to be sent
                                    sendPromise.get_future().wait();
                                }
                                else
                                {
                                    std::cout << "SCHED: Discarding packet because connection is closed" << '\n';
                                }
                            }
                        }
                        catch (std::exception& exception)
                        {
                            dumpExceptions(exception);
                            // Cluster has gone offline, reset the connection. Missed packets shouldn't matter too much,
                            // they should be resent by other threads after some time
                            setConnection(nullptr);
                        }

                        // Data existed
                        hadData = true;
                    }
                }

                // Check if there is higher priority data to send
                if (doesHigherPriorityDataExist(priority))
                {
                    // Yes, so start the entire send process again
                    goto reset;  // NOLINT(cppcoreguidelines-avoid-goto,hicpp-avoid-goto)
                }

                // Higher priority data does not exist, so keep sending data from this priority
            }
            while (hadData);
        }
    }
}

void Cluster::pruneSources()
{
    // Iterate every QUEUE_SOURCE_PRUNE_SECONDS seconds until the timer is cancelled or until we stop running
    do  // NOLINT(cppcoreguidelines-avoid-do-while)
    {
        // Acquire the exclusive lock to prevent more data being pushed on while we are pruning
        {
            const std::unique_lock<std::shared_mutex> lock(mutex);

            // Iterate over the priorities
            for (auto& [priority, pMap] : queue)
            {
                // Iterate over the map
                // NOLINTBEGIN(clang-analyzer-security.ArrayBound)
                for (auto iter = pMap.begin(); iter != pMap.end();)
                {
                    // Check if the vector for this source is empty
                    if ((*iter).second->empty())
                    {
                        // Remove this source from the map and continue
                        iter = pMap.erase(iter);
                        continue;
                    }
                    // Manually increment the iterator
                    ++iter;
                }
                // NOLINTEND(clang-analyzer-security.ArrayBound)
            }
        }
    }
    while (bRunning && interruptablePruneTimer.wait_for(std::chrono::milliseconds(QUEUE_SOURCE_PRUNE_MILLISECONDS)));
}

void Cluster::resendMessages()
{
    // Iterate every CLUSTER_RESEND_MESSAGE_INTERVAL_SECONDS seconds until the timer is cancelled or until we stop
    // running
    while (bRunning &&
           interruptableResendTimer.wait_for(std::chrono::milliseconds(CLUSTER_RESEND_MESSAGE_INTERVAL_MILLISECONDS)))
    {
        // Check for jobs that need to be resubmitted
        checkUnsubmittedJobs();

        // Check for jobs that need to be cancelled
        checkCancellingJobs();

        // Check for jobs that need to be deleted
        checkDeletingJobs();
    }
}

auto Cluster::doesHigherPriorityDataExist(Message::Priority maxPriority) -> bool
{
    for (const auto& [priority, pMap] : queue)
    {
        // Only check priorities higher than maxPriority
        if (static_cast<int>(priority) >= static_cast<int>(maxPriority))
        {
            return false;
        }

        // Iterate over the map
        for (auto iter = pMap.begin(); iter != pMap.end();)
        {
            // Check if the vector for this source is empty
            if (!(*iter).second->empty())
            {
                // Data exists
                return true;
            }

            // Increment the iterator
            ++iter;
        }
    }

    return false;
}

void Cluster::updateJob(Message& message)
{
    // Read the details from the message
    auto jobId   = message.pop_uint();
    auto what    = message.pop_string();
    auto status  = message.pop_uint();
    auto details = message.pop_string();

    // Create a database connection
    auto database = MySqlConnector();

    // Get the tables
    const schema::JobserverJob jobTable;
    const schema::JobserverJobhistory jobHistoryTable;

    // Todo: Verify the job id belongs to this cluster, and that the status is valid

    // Add the new job history record
    try
    {
        [[maybe_unused]] auto result =
            database->run(insert_into(jobHistoryTable)
                              .set(jobHistoryTable.jobId     = jobId,
                                   jobHistoryTable.timestamp = std::chrono::system_clock::now(),
                                   jobHistoryTable.what      = what,
                                   jobHistoryTable.state     = status,
                                   jobHistoryTable.details   = details));
        // For insert operations, if we get here without exception, it succeeded
    }
    catch (const std::exception& e)
    {
        std::cerr << "WARNING: DB - Failed to insert job history record for job " << jobId << " with status " << status
                  << ": " << e.what() << '\n';
    }

    // Check if this status update was the final job complete status, then try to cache the file list by listing the
    // files for the job. If for some reason this call fails internally (Maybe cluster is under load, or network issues)
    // then the next time the HTTP side requests files for the job the file list will be cached
    if (what == "_job_completion_")
    {
        auto jobResults =
            database->run(select(all_of(jobTable)).from(jobTable).where(jobTable.id == static_cast<uint64_t>(jobId)));

        // Check that a job was actually found
        if (jobResults.empty())
        {
            throw std::runtime_error("Unable to find job with ID " + std::to_string(jobId) +
                                     " for application job_controller");
        }

        auto bundle = std::string{(&jobResults.front())->bundle};

        // Launch the file list in a new thread to prevent locking up the websocket. This is an internal system
        // operation and can run in the background, rather than on the websocket message handling thread.
        // We pass parameters by copy here intentionally, not by reference.
        std::thread([bundle, jobId, this] {
            ::handleFileList(app, shared_from_this(), bundle, jobId, true, "", nullptr);
        }).detach();
    }
}

namespace {
auto getJobsByMostRecentStatus(const std::vector<uint32_t>& states, const std::string& cluster)
{
    /*
     Finds all jobs currently in a state provided by the states argument that are older than
     CLUSTER_RECENT_STATE_JOB_IGNORE_SECONDS, for the current cluster
    */

    // Create a database connection
    auto database = MySqlConnector();

    // Get the tables
    const schema::JobserverJob jobTable;
    const schema::JobserverJobhistory jobHistoryTable;

    // Find any jobs with a state in states older than 60 seconds
    auto jobResults = database->run(
        select(all_of(jobTable))
            .from(jobTable)
            .where(jobTable.cluster == cluster and
                   jobTable.id ==
                       select(jobHistoryTable.jobId)
                           .from(jobHistoryTable)
                           .where(jobHistoryTable.state.in(sqlpp::value_list(states)) and
                                  jobHistoryTable.id ==
                                      select(jobHistoryTable.id)
                                          .from(jobHistoryTable)
                                          .where(jobHistoryTable.jobId == jobTable.id and
                                                 jobHistoryTable.timestamp <=
                                                     std::chrono::system_clock::now() -
                                                         std::chrono::seconds(CLUSTER_RECENT_STATE_JOB_IGNORE_SECONDS))
                                          .order_by(jobHistoryTable.timestamp.desc())
                                          .limit(1U))));

    return jobResults;
}
}  // namespace

void Cluster::checkUnsubmittedJobs()
{
    // Check if the cluster is online and resend the submit messages
    if (!isOnline())
    {
        return;
    }

    // Get all jobs where the most recent job history is
    // pending or submitting and is more than a minute old
    auto states = std::vector<uint32_t>(
        {static_cast<uint32_t>(JobStatus::PENDING), static_cast<uint32_t>(JobStatus::SUBMITTING)});

    auto jobs = getJobsByMostRecentStatus(states, this->getName());

    auto database = MySqlConnector();
    const schema::JobserverJobhistory jobHistoryTable;

    // Resubmit any jobs that matched
    for (const auto& job : jobs)
    {
        std::cout << "Resubmitting: " << job.id << '\n';

        // Submit the job to the cluster
        auto msg =
            Message(SUBMIT_JOB, Message::Priority::Medium, std::to_string(job.id) + "_" + std::string{job.cluster});
        msg.push_uint(job.id);
        msg.push_string(job.bundle);
        msg.push_string(job.parameters);
        sendMessage(msg);

        // Check that the job status is submitting and update it if not
        auto jobHistoryResults = database->run(select(all_of(jobHistoryTable))
                                                   .from(jobHistoryTable)
                                                   .where(jobHistoryTable.jobId == job.id)
                                                   .order_by(jobHistoryTable.timestamp.desc())
                                                   .limit(1U));

        const auto* dbHistory = &jobHistoryResults.front();
        if (static_cast<uint32_t>(dbHistory->state) == static_cast<uint32_t>(JobStatus::PENDING))
        {
            // The current state is pending for this job, so update the status to submitting
            try
            {
                [[maybe_unused]] auto result =
                    database->run(insert_into(jobHistoryTable)
                                      .set(jobHistoryTable.jobId     = job.id,
                                           jobHistoryTable.timestamp = std::chrono::system_clock::now(),
                                           jobHistoryTable.what      = SYSTEM_SOURCE,
                                           jobHistoryTable.state     = static_cast<uint32_t>(JobStatus::SUBMITTING),
                                           jobHistoryTable.details   = "Job submitting"));
                // Insert succeeded if we get here without exception
            }
            catch (const std::exception& e)
            {
                std::cerr << "WARNING: DB - Failed to update job " << job.id
                          << " status from PENDING to SUBMITTING: " << e.what() << '\n';
            }
        }
    }
}

void Cluster::checkCancellingJobs()
{
    // Check if the cluster is online and resend the submit messages
    if (!isOnline())
    {
        return;
    }

    // Get all jobs where the most recent job history is
    // deleting and is more than a minute old
    auto states = std::vector<uint32_t>({static_cast<uint32_t>(JobStatus::CANCELLING)});

    auto jobs = getJobsByMostRecentStatus(states, this->getName());

    // Resubmit any jobs that matched
    for (const auto& job : jobs)
    {
        std::cout << "Recancelling: " << job.id << '\n';

        // Ask the cluster to cancel the job
        auto msg =
            Message(CANCEL_JOB, Message::Priority::Medium, std::to_string(job.id) + "_" + std::string{job.cluster});
        msg.push_uint(job.id);
        sendMessage(msg);
    }
}

void Cluster::checkDeletingJobs()
{
    // Check if the cluster is online and resend the submit messages
    if (!isOnline())
    {
        return;
    }

    // Get all jobs where the most recent job history is
    // deleting and is more than a minute old
    auto states = std::vector<uint32_t>({static_cast<uint32_t>(JobStatus::DELETING)});

    auto jobs = getJobsByMostRecentStatus(states, this->getName());

    // Resubmit any jobs that matched
    for (const auto& job : jobs)
    {
        std::cout << "Redeleting: " << job.id << '\n';

        // Ask the cluster to delete the job
        auto msg =
            Message(DELETE_JOB, Message::Priority::Medium, std::to_string(job.id) + "_" + std::string{job.cluster});
        msg.push_uint(job.id);
        sendMessage(msg);
    }
}

void Cluster::handleFileListError(Message& message)
{
    auto uuid   = message.pop_string();
    auto detail = message.pop_string();

    // Acquire the lock
    const std::unique_lock<std::mutex> fileListMapDeletionLock(app->getFileListMapDeletionLockMutex());

    // Check that the uuid is valid
    auto fileListMap = app->getFileListMap();
    if (fileListMap->find(uuid) == fileListMap->end())
    {
        return;
    }

    auto flObj = fileListMap->at(uuid);

    // Set the error
    flObj->errorDetails = detail;
    flObj->error        = true;

    // Trigger the file transfer event
    flObj->dataReady = true;
    flObj->dataCV.notify_one();
}

void Cluster::handleFileList(Message& message)
{
    auto uuid = message.pop_string();

    // Acquire the lock
    const std::unique_lock<std::mutex> fileListMapDeletionLock(app->getFileListMapDeletionLockMutex());

    // Check that the uuid is valid
    auto fileListMap = app->getFileListMap();
    if (fileListMap->find(uuid) == fileListMap->end())
    {
        return;
    }

    auto flObj = fileListMap->at(uuid);

    // Get the number of files in the message
    auto numFiles = message.pop_uint();

    // Iterate over the files and add them to the file list map
    for (auto i = 0U; i < numFiles; i++)
    {
        sFile file;
        // todo: Need to get file permissions
        file.fileName    = message.pop_string();
        file.isDirectory = message.pop_bool();
        file.fileSize    = message.pop_ulong();

        // Add the file to the list
        flObj->files.push_back(file);
    }

    // Tell the HTTP side that the data is ready
    flObj->dataReady = true;
    flObj->dataCV.notify_one();
}
