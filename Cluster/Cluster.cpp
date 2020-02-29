//
// Created by lewis on 2/27/20.
//

#include "Cluster.h"

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

Cluster::Cluster(std::string name, ClusterManager* pClusterManager) {
    this->name = std::move(name);
    this->pClusterManager = pClusterManager;

    // Create the list of priorities in order
    for (uint32_t i = (uint32_t) Message::Priority::Highest; i <= (uint32_t) Message::Priority::Lowest; i++)
        queue.push_back(std::map<std::string, boost::lockfree::queue<std::vector<uint8_t>*>>());

    // Start the scheduler thread
    std::thread schedulerThread([this] {
        this->run();
    });
}

void Cluster::connect(std::string token) {
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

    boost::lockfree::queue<std::vector<uint8_t>*>* pQueue;
    // Lock the access mutex to check if the source exists in the map
    {
        std::unique_lock<std::shared_mutex> lk(mutex_);
        if (!pMap->contains(source))
            (*pMap)[source] = boost::lockfree::queue<std::vector<uint8_t>*>();

        pQueue = &(*pMap)[source];
    }

    // Push the data on to the queue
    pQueue->push(data);
}

void Cluster::run() {
    while (true) {
        // Wait for data to be ready to send
        std::unique_lock<std::mutex> lk(mutex_);
    }
}


//     auto o = std::make_shared<WsServer::OutMessage>(data.size());
//    std::ostream_iterator<uint8_t> iter(*o.get());
//    std::copy(data.begin(), data.end(), iter);