//
// Created by lewis on 2/27/20.
//

#include "Cluster.h"
#include "../DB/ClusterDB.h"
#include "../DB/MySqlConnector.h"
#include "../HTTP/HttpServer.h"
#include "../HTTP/Utils/HandleFileList.h"
#include "../Lib/JobStatus.h"
#include "../Lib/jobserver_schema.h"

#include <client_http.hpp>
#include <folly/Uri.h>
#include <iostream>

// Packet Queue is a:
//  list of priorities - doesn't need any sync because it never changes
//      -> map of sources - needs sync when adding/removing sources
//          -> vector of packets - make this a MPSC queue

// When the number of bytes in a packet of vectors exceeds some amount, a message should be sent that stops more
// packets from being sent, when the vector then falls under some threshold

// Track sources in the map, add them when required - delete them after some amount (1 minute?) of inactivity.

// Send sources round robin, starting from the highest priority

// Define a global map that can be used for storing information about file downloads
// NOLINTNEXTLINE(cert-err58-cpp)
const std::shared_ptr<folly::ConcurrentHashMap<std::string, std::shared_ptr<FileDownload>>> fileDownloadMap = std::make_shared<folly::ConcurrentHashMap<std::string, std::shared_ptr<FileDownload>>>();

// Define a global map that can be used for storing information about file lists
// NOLINTNEXTLINE(cert-err58-cpp)
const std::shared_ptr<folly::ConcurrentHashMap<std::string, std::shared_ptr<sFileList>>> fileListMap = std::make_shared<folly::ConcurrentHashMap<std::string, std::shared_ptr<sFileList>>>();

// Define a mutex that can be used for safely removing entries from the fileDownloadMap
// NOLINTNEXTLINE(cppcoreguidelines-avoid-non-const-global-variables)
std::mutex fileDownloadMapDeletionLockMutex;

// Define a mutex that can be used for synchronising pause/resume messages
// NOLINTNEXTLINE(cppcoreguidelines-avoid-non-const-global-variables)
std::mutex fileDownloadPauseResumeLockMutex;

// Define a mutex that can be used for safely removing entries from the fileListMap
// NOLINTNEXTLINE(cppcoreguidelines-avoid-non-const-global-variables)
std::mutex fileListMapDeletionLockMutex;

Cluster::Cluster(std::shared_ptr<sClusterDetails> details) : pClusterDetails(std::move(details)) {
    // Create the list of priorities in order
    for (auto i = static_cast<uint32_t>(Message::Priority::Highest); i <= static_cast<uint32_t>(Message::Priority::Lowest); i++) {
        queue.emplace_back();
    }

#ifndef BUILD_TESTS
    // Start the scheduler thread
    schedulerThread = std::thread([this] {
        this->run();
    });

    // Start the prune thread
    pruneThread = std::thread([this] {
        this->pruneSources();
    });

    // Start the resend thread if this is a master cluster
    if (getRole() == eRole::master) {
        resendThread = std::thread([this] {
            this->resendMessages();
        });
    }
#endif
}

void Cluster::handleMessage(Message &message) {
    // Check if the message can be handled by the cluster database
    if (ClusterDB::maybeHandleClusterDBMessage(message, shared_from_this())) {
        return;
    }

    auto msgId = message.getId();

    switch (msgId) {
        case UPDATE_JOB:
            this->updateJob(message);
            break;
        case FILE_LIST:
            Cluster::handleFileList(message);
            break;
        case FILE_LIST_ERROR:
            Cluster::handleFileListError(message);
            break;
        default:
            std::cout << "Got invalid message ID " << msgId << " from " << this->getName() << std::endl;
    }
}

void Cluster::setConnection(const std::shared_ptr<WsServer::Connection>& pCon) {
    this->pConnection = pCon;

    if (pCon != nullptr && getRole() == eRole::master) {
        // See if there are any pending jobs that should be sent
        checkUnsubmittedJobs();

        // See if there are any cancelling jobs that should be sent
        checkCancellingJobs();

        // See if there are any deleting jobs that should be sent
        checkDeletingJobs();
    }
}

void Cluster::queueMessage(std::string source, const std::shared_ptr<std::vector<uint8_t>>& pData, Message::Priority priority) {
    // Get a pointer to the relevant map
    auto *pMap = &queue[priority];

    // Lock the access mutex to check if the source exists in the map
    {
        std::shared_lock<std::shared_mutex> lock(mutex_);

        // Make sure that this source exists in the map
        auto sQueue = std::make_shared<folly::UMPSCQueue<std::shared_ptr<std::vector<uint8_t>>, false>>();

        // Make sure that the source is in the map
        pMap->try_emplace(source, sQueue);

        // Write the data in the queue
        (*pMap)[source]->enqueue(pData);

        // Trigger the new data event to start sending
        this->dataReady = true;
        dataCV.notify_one();
    }
}


#ifndef BUILD_TESTS
[[noreturn]] void Cluster::pruneSources() {
    // Iterate forever
    while (true) {
        // Wait 1 minute until the next prune
        std::this_thread::sleep_for(std::chrono::seconds(QUEUE_SOURCE_PRUNE_SECONDS));
#else

void Cluster::pruneSources() {
#endif
    // Acquire the exclusive lock to prevent more data being pushed on while we are pruning
    {
        std::unique_lock<std::shared_mutex> lock(mutex_);

        // Iterate over the priorities
        for (auto &priority : queue) {
            // Get a pointer to the relevant map
            auto *pMap = &priority;

            // Iterate over the map
            for (auto iter = pMap->begin(); iter != pMap->end();) {
                // Check if the vector for this source is empty
                if ((*iter).second->empty()) {
                    // Remove this source from the map and continue
                    iter = pMap->erase(iter);
                    continue;
                }
                // Manually increment the iterator
                ++iter;
            }
        }
    }
#ifndef BUILD_TESTS
    }
#endif
}

#ifndef BUILD_TESTS
[[noreturn]] void Cluster::run() { // NOLINT(readability-function-cognitive-complexity)
    // Iterate forever
    while (true) {
#else

void Cluster::run() { // NOLINT(readability-function-cognitive-complexity)
#endif
    {
        std::unique_lock<std::mutex> lock(dataCVMutex);

#ifndef BUILD_TESTS
        // Wait for data to be ready to send
        dataCV.wait(lock, [this] { return this->dataReady; });
#endif
        // Reset the condition
        this->dataReady = false;
    }

    reset:

    // Iterate over the priorities
    for (auto priority = queue.begin(); priority != queue.end(); priority++) {

        // Get a pointer to the relevant map
        auto *pMap = &(*priority);

        // Get the current priority
        auto currentPriority = priority - queue.begin();

        // While there is still data for this priority, send it
        bool hadData = false;
        do {
            hadData = false;

            std::shared_lock<std::shared_mutex> lock(mutex_);
            // Iterate over the map
            for (auto iter = pMap->begin(); iter != pMap->end(); ++iter) {
                // Check if the vector for this source is empty
                if (!(*iter).second->empty()) {

                    // Pop the next item from the queue
                    auto data = (*iter).second->try_dequeue();

                    try {
                        // data should never be null as we're checking for empty
                        if (data) {
                            // Convert the message
                            auto outMessage = std::make_shared<WsServer::OutMessage>((*data)->size());
                            std::copy((*data)->begin(), (*data)->end(), std::ostream_iterator<uint8_t>(*outMessage));

                            // Send the message on the websocket
                            if (pConnection != nullptr) {
                                std::promise<void> sendPromise;
                                pConnection->send(
                                    outMessage,
                                    [this, &sendPromise](const SimpleWeb::error_code &errorCode) {
                                        // Data has been sent, set the promise
                                        sendPromise.set_value();

                                        // Kill the connection only if the error was not indicating success
                                        if (!errorCode){
                                            return;
                                        }

                                        pConnection->close();
                                        pConnection = nullptr;

                                        ClusterManager::reportWebsocketError(shared_from_this(), errorCode);
                                    },
                                    // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
                                    130
                                );

                                // Wait for the data to be sent
                                sendPromise.get_future().wait();
                            } else {
                                std::cout << "SCHED: Discarding packet because connection is closed" << std::endl;
                            }
                        }
                    } catch (std::exception& exception) {
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
            if (doesHigherPriorityDataExist(currentPriority)) {
                // Yes, so start the entire send process again
                goto reset; // NOLINT(cppcoreguidelines-avoid-goto,hicpp-avoid-goto)
            }

            // Higher priority data does not exist, so keep sending data from this priority
        } while (hadData);
    }
#ifndef BUILD_TESTS
    }
#endif
}

auto Cluster::doesHigherPriorityDataExist(uint64_t maxPriority) -> bool {
    for (auto priority = queue.begin(); priority != queue.end(); priority++) {
        // Get a pointer to the relevant map
        auto *pMap = &(*priority);

        // Check if the current priority is greater or equal to max priority and return false if not.
        auto currentPriority = priority - queue.begin();
        if (currentPriority >= maxPriority) {
            return false;
        }

        // Iterate over the map
        for (auto iter = pMap->begin(); iter != pMap->end();) {
            // Check if the vector for this source is empty
            if (!(*iter).second->empty()) {
                // It'iter not empty so data does exist
                return true;
            }

            // Increment the iterator
            ++iter;
        }
    }

    return false;
}

void Cluster::updateJob(Message &message) {
    // Read the details from the message
    auto jobId = message.pop_uint();
    auto what = message.pop_string();
    auto status = message.pop_uint();
    auto details = message.pop_string();

    // Create a database connection
    auto database = MySqlConnector();

    // Get the tables
    schema::JobserverJob jobTable;
    schema::JobserverJobhistory jobHistoryTable;

    // Todo: Verify the job id belongs to this cluster, and that the status is valid

    // Add the new job history record
    database->run(
            insert_into(jobHistoryTable)
                    .set(
                            jobHistoryTable.jobId = jobId,
                            jobHistoryTable.timestamp = std::chrono::system_clock::now(),
                            jobHistoryTable.what = what,
                            jobHistoryTable.state = status,
                            jobHistoryTable.details = details
                    )
    );

    // Check if this status update was the final job complete status, then try to cache the file list by listing the
    // files for the job. If for some reason this call fails internally (Maybe cluster is under load, or network issues)
    // then the next time the HTTP side requests files for the job the file list will be cached
    if (what == "_job_completion_") {
        auto jobResults = database->run(
                select(all_of(jobTable))
                        .from(jobTable)
                        .where(
                                jobTable.id == static_cast<uint64_t>(jobId)
                        )
        );

        // Check that a job was actually found
        if (jobResults.empty()) {
            throw std::runtime_error(
                    "Unable to find job with ID " + std::to_string(jobId) + " for application job_controller");
        }

        auto bundle = std::string{(&jobResults.front())->bundle};

        // Launch the file list in a new thread to prevent locking up the websocket. This is an internal system
        // operation and can run in the background, rather than on the websocket message handling thread.
        // We pass parameters by copy here intentionally, not by reference.
        std::thread([bundle, jobId, this] {
            ::handleFileList(shared_from_this(), bundle, jobId, true, "", nullptr);
        }).detach();
    }
}

auto Cluster::isOnline() -> bool {
    return pConnection != nullptr;
}

[[noreturn]] void Cluster::resendMessages() {
    // Iterate forever
    while (true) {
        // Wait 1 minute until the next check
        std::this_thread::sleep_for(std::chrono::seconds(CLUSTER_RESEND_MESSAGE_INTERVAL_SECONDS));
        
        // Check for jobs that need to be resubmitted
        checkUnsubmittedJobs();

        // Check for jobs that need to be cancelled
        checkCancellingJobs();

        // Check for jobs that need to be deleted
        checkDeletingJobs();
    }
}

auto getJobsByMostRecentStatus(const std::vector<uint32_t>& states, const std::string& cluster) {
    /*
     Finds all jobs currently in a state provided by the states argument that are older than
     CLUSTER_RECENT_STATE_JOB_IGNORE_SECONDS, for the current cluster
    */

    // Create a database connection
    auto database = MySqlConnector();

    // Get the tables
    schema::JobserverJob jobTable;
    schema::JobserverJobhistory jobHistoryTable;

    // Find any jobs with a state in states older than 60 seconds
    auto jobResults =
            database->run(
                    select(all_of(jobTable))
                            .from(jobTable)
                            .where(
                                    jobTable.cluster == cluster
                                    and
                                    jobTable.id == select(jobHistoryTable.jobId)
                                            .from(jobHistoryTable)
                                            .where(
                                                    jobHistoryTable.state.in(
                                                            sqlpp::value_list(
                                                                    states
                                                            )
                                                    )
                                                    and
                                                    jobHistoryTable.id == select(jobHistoryTable.id)
                                                            .from(jobHistoryTable)
                                                            .where(
                                                                    jobHistoryTable.jobId == jobTable.id
                                                                    and
                                                                    jobHistoryTable.timestamp <=
                                                                    std::chrono::system_clock::now() -
                                                                    std::chrono::seconds(CLUSTER_RECENT_STATE_JOB_IGNORE_SECONDS)
                                                            )
                                                            .order_by(jobHistoryTable.timestamp.desc())
                                                            .limit(1U)
                                            )
                            )
            );

    return jobResults;
}

void Cluster::checkUnsubmittedJobs() {
    // Check if the cluster is online and resend the submit messages
    if (!isOnline()) {
        return;
    }

    // Get all jobs where the most recent job history is
    // pending or submitting and is more than a minute old
    auto states = std::vector<uint32_t>(
            {
                    static_cast<uint32_t>(PENDING),
                    static_cast<uint32_t>(SUBMITTING)
            }
    );

    auto jobs = getJobsByMostRecentStatus(states, this->getName());

    auto database = MySqlConnector();
    schema::JobserverJobhistory jobHistoryTable;

    // Resubmit any jobs that matched
    for (const auto &job : jobs) {
        std::cout << "Resubmitting: " << job.id << std::endl;

        // Submit the job to the cluster
        auto msg = Message(SUBMIT_JOB, Message::Priority::Medium,
                           std::to_string(job.id) + "_" + std::string{job.cluster});
        msg.push_uint(job.id);
        msg.push_string(job.bundle);
        msg.push_string(job.parameters);
        msg.send(shared_from_this());

        // Check that the job status is submitting and update it if not
        auto jobHistoryResults =
                database->run(
                        select(all_of(jobHistoryTable))
                                .from(jobHistoryTable)
                                .where(jobHistoryTable.jobId == job.id)
                                .order_by(jobHistoryTable.timestamp.desc())
                                .limit(1U)
                );

        const auto *dbHistory = &jobHistoryResults.front();
        if (static_cast<uint32_t>(dbHistory->state) == static_cast<uint32_t>(JobStatus::PENDING)) {
            // The current state is pending for this job, so update the status to submitting
            database->run(
                    insert_into(jobHistoryTable)
                            .set(
                                    jobHistoryTable.jobId = job.id,
                                    jobHistoryTable.timestamp = std::chrono::system_clock::now(),
                                    jobHistoryTable.what = SYSTEM_SOURCE,
                                    jobHistoryTable.state = static_cast<uint32_t>(JobStatus::SUBMITTING),
                                    jobHistoryTable.details = "Job submitting"
                            )
            );
        }
    }
}

void Cluster::checkCancellingJobs() {
    // Check if the cluster is online and resend the submit messages
    if (!isOnline()) {
        return;
    }

    // Get all jobs where the most recent job history is
    // deleting and is more than a minute old
    auto states = std::vector<uint32_t>(
            {
                    static_cast<uint32_t>(CANCELLING)
            }
    );

    auto jobs = getJobsByMostRecentStatus(states, this->getName());

    // Resubmit any jobs that matched
    for (const auto &job : jobs) {
        std::cout << "Recancelling: " << job.id << std::endl;

        // Ask the cluster to cancel the job
        auto msg = Message(CANCEL_JOB, Message::Priority::Medium,
                        std::to_string(job.id) + "_" + std::string{job.cluster});
        msg.push_uint(job.id);
        msg.send(shared_from_this());
    }
}

void Cluster::checkDeletingJobs() {
    // Check if the cluster is online and resend the submit messages
    if (!isOnline()) {
        return;
    }

    // Get all jobs where the most recent job history is
    // deleting and is more than a minute old
    auto states = std::vector<uint32_t>(
            {
                    static_cast<uint32_t>(DELETING)
            }
    );

    auto jobs = getJobsByMostRecentStatus(states, this->getName());

    // Resubmit any jobs that matched
    for (const auto &job : jobs) {
        std::cout << "Redeleting: " << job.id << std::endl;

        // Ask the cluster to delete the job
        auto msg = Message(DELETE_JOB, Message::Priority::Medium,
                        std::to_string(job.id) + "_" + std::string{job.cluster});
        msg.push_uint(job.id);
        msg.send(shared_from_this());
    }
}

void Cluster::handleFileListError(Message &message) {
    auto uuid = message.pop_string();
    auto detail = message.pop_string();

    // Acquire the lock
    std::unique_lock<std::mutex> fileListMapDeletionLock(fileListMapDeletionLockMutex);

    // Check that the uuid is valid
    if (fileListMap->find(uuid) == fileListMap->end()) {
        return;
    }

    auto flObj = (*fileListMap)[uuid];

    // Set the error
    flObj->errorDetails = detail;
    flObj->error = true;

    // Trigger the file transfer event
    flObj->dataReady = true;
    flObj->dataCV.notify_one();
}

void Cluster::handleFileList(Message &message) {
    auto uuid = message.pop_string();

    // Acquire the lock
    std::unique_lock<std::mutex> fileListMapDeletionLock(fileListMapDeletionLockMutex);

    // Check that the uuid is valid
    if (fileListMap->find(uuid) == fileListMap->end()) {
        return;
    }

    auto flObj = (*fileListMap)[uuid];

    // Get the number of files in the message
    auto numFiles = message.pop_uint();

    // Iterate over the files and add them to the file list map
    for (auto i = 0; i < numFiles; i++) {
        sFile file;
        // todo: Need to get file permissions
        file.fileName = message.pop_string();
        file.isDirectory = message.pop_bool();
        file.fileSize = message.pop_ulong();

        // Add the file to the list
        flObj->files.push_back(file);
    }

    // Tell the HTTP side that the data is ready
    flObj->dataReady = true;
    flObj->dataCV.notify_one();
}