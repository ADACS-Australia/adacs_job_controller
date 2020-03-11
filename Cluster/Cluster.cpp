//
// Created by lewis on 2/27/20.
//

#include "Cluster.h"
#include "../DB/MySqlConnector.h"
#include "../Lib/jobserver_schema.h"

#include <utility>
#include <iostream>

// Queue is a:
//  list of priorities - doesn't need any sync because it never changes
//      -> map of sources - needs sync when adding/removing sources
//          -> vector of packets - make this a MPSC queue

// When the number of bytes in a packet of vectors exceeds some amount, a message should be sent that stops more
// packets from being sent, when the vector then falls under some threshold

// Track sources in the map, add them when required - delete them after some amount (1 minute?) of inactivity.

// Send sources round robin, starting from the highest priority

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
}

void Cluster::connect(const std::string& token) {
    std::cout << "Attempting to connect cluster " << name << " with token " << token << std::endl;
    std::system(("cd ./utils/keyserver; ./keyserver " + name + " " + token).c_str());
}

void Cluster::setConnection(WsServer::Connection *pConnection) {
    this->pConnection = pConnection;
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
        std::this_thread::sleep_for(std::chrono::seconds(5));

        // Aquire the exclusive lock to prevent more data being pushed on while we are pruning
        {
            std::unique_lock<std::shared_mutex> lock(mutex_);

            // Iterate over the priorities
            for (auto & p : queue) {
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

                        // data should never be null as we're checking for empty
                        if (data) {
                            // Convert the message
                            auto o = std::make_shared<WsServer::OutMessage>((*data)->size());
                            std::ostream_iterator<uint8_t> iter(*o);
                            std::copy((*data)->begin(), (*data)->end(), iter);

                            // Send the message on the websocket
                            pConnection->send(o, nullptr, 130);
                        }

                        // Clean up the message
                        delete (*data);

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

void Cluster::handleMessage(Message &message) {
    auto msgId = message.getId();

    switch (msgId) {
        case UPDATE_JOB:
            this->updateJob(message);
            break;
        default:
            std::cout << "Got invalid message ID " << msgId << " from " << name << std::endl;
    };
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
