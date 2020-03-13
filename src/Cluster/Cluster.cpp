//
// Created by lewis on 2/27/20.
//

#include "Cluster.h"
#include "../DB/MySqlConnector.h"
#include "../Lib/jobserver_schema.h"
#include "../Lib/JobStatus.h"

#include <utility>
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
folly::ConcurrentHashMap<std::string, sFileDownload *> fileDownloadMap;

// Define a global map that can be used for storing information about file lists
folly::ConcurrentHashMap<std::string, sFileList *> fileListMap;

using namespace schema;

Cluster::Cluster(std::string name, ClusterManager *pClusterManager) {
    this->name = std::move(name);
    this->pClusterManager = pClusterManager;

    // Create the list of priorities in order
    for (auto i = (uint32_t) Message::Priority::Highest; i <= (uint32_t) Message::Priority::Lowest; i++)
        queue.emplace_back();

    // Start the scheduler thread
    schedulerThread = std::thread([this] {
        this->run();
    });

    // Start the prune thread
    pruneThread = std::thread([this] {
        this->pruneSources();
    });

    // Start the resend thread
    resendThread = std::thread([this] {
        this->resendMessages();
    });
}

void Cluster::handleMessage(Message &message) {
    auto msgId = message.getId();

    switch (msgId) {
        case UPDATE_JOB:
            this->updateJob(message);
            break;
        case FILE_ERROR:
            Cluster::handleFileError(message);
            break;
        case FILE_DETAILS:
            Cluster::handleFileDetails(message);
            break;
        case FILE_CHUNK:
            this->handleFileChunk(message);
            break;
        case FILE_LIST:
            this->handleFileList(message);
            break;
        default:
            std::cout << "Got invalid message ID " << msgId << " from " << name << std::endl;
    };
}

void Cluster::connect(const std::string &token) {
    std::cout << "Attempting to connect cluster " << name << " with token " << token << std::endl;
    std::system(("cd ./utils/keyserver; ./keyserver " + name + " " + token).c_str());
}

void Cluster::setConnection(WsServer::Connection *pConnection) {
    this->pConnection = pConnection;

    if (pConnection != nullptr) {
        // See if there are any pending jobs that should be sent
        checkUnsubmittedJobs();
    }
}

void Cluster::queueMessage(std::string source, std::vector<uint8_t> *data, Message::Priority priority) {
    // Copy the message
    auto pData = new std::vector<uint8_t>(*data);

    // Get a pointer to the relevant map
    auto pMap = &queue[priority];

    // Lock the access mutex to check if the source exists in the map
    {
        std::shared_lock<std::shared_mutex> lock(mutex_);

        // Make sure that this source exists in the map
        pMap->try_emplace(source, new folly::UMPSCQueue<std::vector<uint8_t> *, false, 8>());

        // Write the data in the queue
        (*pMap)[source]->enqueue(pData);

        // Trigger the new data event to start sending
        this->dataReady = true;
        dataCV.notify_one();
    }
}

void Cluster::pruneSources() {
    // Iterate forever
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wmissing-noreturn"
    while (true) {
        // Wait 1 minute until the next prune
        std::this_thread::sleep_for(std::chrono::seconds(60));

        // Aquire the exclusive lock to prevent more data being pushed on while we are pruning
        {
            std::unique_lock<std::shared_mutex> lock(mutex_);

            // Iterate over the priorities
            for (auto &p : queue) {
                // Get a pointer to the relevant map
                auto pMap = &p;

                // Iterate over the map
                for (auto s = pMap->begin(); s != pMap->end();) {
                    // Check if the vector for this source is empty
                    if ((*s).second->empty()) {
                        // Destroy the queue
                        delete (*s).second;

                        // Remove this source from the map and continue
                        s = pMap->erase(s);
                        continue;
                    }
                    // Manually increment the iterator
                    ++s;
                }
            }
        }
    }
#pragma clang diagnostic pop
}

void Cluster::run() {
    // Iterate forever
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wmissing-noreturn"
    while (true) {
        {
            std::unique_lock<std::mutex> lock(dataCVMutex);

            // Wait for data to be ready to send
            dataCV.wait(lock, [this] { return this->dataReady; });

            // Reset the condition
            this->dataReady = false;
        }

        reset:

        // Iterate over the priorities
        for (auto p = queue.begin(); p != queue.end(); p++) {

            // Get a pointer to the relevant map
            auto pMap = &(*p);

            // Get the current priority
            auto currentPriority = p - queue.begin();

            // While there is still data for this priority, send it
            bool hadData;
            do {
                hadData = false;

                std::shared_lock<std::shared_mutex> lock(mutex_);
                // Iterate over the map
                for (auto s = pMap->begin(); s != pMap->end(); ++s) {
                    // Check if the vector for this source is empty
                    if (!(*s).second->empty()) {

                        // Pop the next item from the queue
                        auto data = (*s).second->try_dequeue();

                        try {
                            // data should never be null as we're checking for empty
                            if (data) {
                                // Convert the message
                                auto o = std::make_shared<WsServer::OutMessage>((*data)->size());
                                std::ostream_iterator<uint8_t> iter(*o);
                                std::copy((*data)->begin(), (*data)->end(), iter);

                                // Send the message on the websocket
                                if (pConnection)
                                    pConnection->send(o, nullptr, 130);
                                else
                                    std::cout << "SCHED: Discarding packet because connection is closed";

                                // Clean up the message
                                delete (*data);
                            }
                        } catch (...) {
                            // Cluster has gone offline, reset the connection. Missed packets shouldn't matter too much,
                            // they should be resent by other threads after some time
                            setConnection(nullptr);
                        }

                        // Data existed
                        hadData = true;
                    }
                }

                // Check if there is higher priority data to send
                if (doesHigherPriorityDataExist(currentPriority))
                    // Yes, so start the entire send process again
                    goto reset;

                // Higher priority data does not exist, so keep sending data from this priority
            } while (hadData);
        }
    }
#pragma clang diagnostic pop
}

bool Cluster::doesHigherPriorityDataExist(uint64_t maxPriority) {
    for (auto p = queue.begin(); p != queue.end(); p++) {
        // Get a pointer to the relevant map
        auto pMap = &(*p);

        // Check if the current priority is greater or equal to max priority and return false if not.
        auto currentPriority = p - queue.begin();
        if (currentPriority >= maxPriority)
            return false;

        // Iterate over the map
        for (auto s = pMap->begin(); s != pMap->end();) {
            // Check if the vector for this source is empty
            if (!(*s).second->empty()) {
                // It's not empty so data does exist
                return true;
            }

            // Increment the iterator
            ++s;
        }
    }

    return false;
}

void Cluster::updateJob(Message &message) {
    // Read the details from the message
    auto jobId = message.pop_uint();
    auto status = message.pop_uint();
    auto details = message.pop_string();

    // Create a database connection
    auto db = MySqlConnector();

    // Get the job history table
    JobserverJobhistory jobHistoryTable;

    // Todo: Verify the job id belongs to this cluster, and that the status is valid

    // Create the first state object
    db->run(
            insert_into(jobHistoryTable)
                    .set(
                            jobHistoryTable.jobId = jobId,
                            jobHistoryTable.timestamp = std::chrono::system_clock::now(),
                            jobHistoryTable.state = status,
                            jobHistoryTable.details = details
                    )
    );
}

bool Cluster::isOnline() {
    return pConnection != nullptr;
}

void Cluster::resendMessages() {
    // Iterate forever
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wmissing-noreturn"
    while (true) {
        // Wait 1 minute until the next check
        std::this_thread::sleep_for(std::chrono::seconds(60));

        // Check for jobs that need to be resubmitted
        checkUnsubmittedJobs();
    }
#pragma clang diagnostic pop
}

void Cluster::checkUnsubmittedJobs() {
    // Check if the cluster is online and resend the submit messages
    if (!isOnline())
        return;

    // Create a database connection
    auto db = MySqlConnector();

    // Get the tables
    JobserverJob jobTable;
    JobserverJobhistory jobHistoryTable;

    // Select all jobTables where the most recent jobHistoryTable is
    // pending or submitting and is more than a minute old

    // Find any jobs in submitting state older than 60 seconds
    auto jobResults =
            db->run(
                    select(all_of(jobTable))
                            .from(jobTable)
                            .where(
                                    jobTable.id == select(jobHistoryTable.jobId)
                                            .from(jobHistoryTable)
                                            .where(
                                                    jobHistoryTable.state.in(
                                                            sqlpp::value_list(
                                                                    std::vector<uint32_t>(
                                                                            {
                                                                                    (uint32_t) PENDING,
                                                                                    (uint32_t) SUBMITTING
                                                                            }
                                                                    )
                                                            )
                                                    )
                                                    and
                                                    jobHistoryTable.id == select(jobHistoryTable.id)
                                                            .from(jobHistoryTable)
                                                            .where(
                                                                    jobHistoryTable.jobId == jobTable.id
                                                                    and
                                                                    jobHistoryTable.timestamp <=
                                                                    std::chrono::system_clock::now() +
                                                                    std::chrono::seconds(60)
                                                            )
                                                            .order_by(jobHistoryTable.timestamp.desc())
                                                            .limit(1u)
                                            )
                            )
            );

    for (auto &job : jobResults) {
        std::cout << "Resubmitting: " << job.id << std::endl;

        // Submit the job to the cluster
        auto msg = Message(SUBMIT_JOB, Message::Priority::Medium,
                           std::to_string(job.id) + "_" + std::string(job.cluster));
        msg.push_uint(job.id);
        msg.push_string(job.bundle);
        msg.push_string(job.parameters);
        msg.send(this);
    }
}

void Cluster::handleFileError(Message &message) {
    auto uuid = message.pop_string();
    auto detail = message.pop_string();

    // Check that the uuid is valid
    if (fileDownloadMap.find(uuid) == fileDownloadMap.end())
        return;

    // Set the error
    fileDownloadMap[uuid]->errorDetails = detail;
    fileDownloadMap[uuid]->error = true;

    // Trigger the file transfer event
    fileDownloadMap[uuid]->dataReady = true;
    fileDownloadMap[uuid]->dataCV.notify_one();
}

void Cluster::handleFileDetails(Message &message) {
    auto uuid = message.pop_string();
    auto fileSize = message.pop_ulong();

    // Check that the uuid is valid
    if (fileDownloadMap.find(uuid) == fileDownloadMap.end())
        return;

    // Set the file size
    fileDownloadMap[uuid]->fileSize = fileSize;
    fileDownloadMap[uuid]->receivedData = true;

    // Trigger the file transfer event
    fileDownloadMap[uuid]->dataReady = true;
    fileDownloadMap[uuid]->dataCV.notify_one();
}
uint64_t max;
void Cluster::handleFileChunk(Message &message) {
    auto uuid = message.pop_string();
    auto chunk = message.pop_bytes();

    // Check that the uuid is valid
    if (fileDownloadMap.find(uuid) == fileDownloadMap.end())
        return;

    fileDownloadMap[uuid]->receivedBytes += chunk.size();

    // Copy the chunk and push it on to the queue
    fileDownloadMap[uuid]->queue.enqueue(new std::vector<uint8_t>(chunk));

    // Trigger the file transfer event
    fileDownloadMap[uuid]->dataReady = true;
    fileDownloadMap[uuid]->dataCV.notify_one();

    if (fileDownloadMap[uuid]->receivedBytes - fileDownloadMap[uuid]->sentBytes > max) {
        max = fileDownloadMap[uuid]->receivedBytes - fileDownloadMap[uuid]->sentBytes;
    }

    if (!fileDownloadMap[uuid]->clientPaused) {
        // Check if our buffer is too big
        if (fileDownloadMap[uuid]->receivedBytes - fileDownloadMap[uuid]->sentBytes > MAX_FILE_BUFFER_SIZE) {
            // Ask the client to pause the file transfer
            fileDownloadMap[uuid]->clientPaused = true;

            auto msg = Message(PAUSE_FILE_CHUNK_STREAM, Message::Priority::Highest, uuid);
            msg.push_string(uuid);
            msg.send(this);
        }
    }
}

void Cluster::handleFileList(Message &message) {
    auto uuid = message.pop_string();

    // Check that the uuid is valid
    if (fileListMap.find(uuid) == fileListMap.end())
        return;

    // Get the number of files in the message
    auto numFiles = message.pop_uint();

    // Iterate over the files and add them to the file list map
    for (auto i = 0; i < numFiles; i++) {
        sFile s;
        // todo: Need to get file permissions
        s.fileName = message.pop_string();
        s.isDirectory = message.pop_bool();
        s.fileSize = message.pop_ulong();

        // Add the file to the list
        fileListMap[uuid]->files.push_back(s);
    }

    // Tell the HTTP side that the data is ready
    fileListMap[uuid]->dataReady = true;
    fileListMap[uuid]->dataCV.notify_one();
}
