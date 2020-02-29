//
// Created by lewis on 2/27/20.
//

#include "Cluster.h"

#include <utility>
#include <iostream>

Cluster::Cluster(std::string name, ClusterManager* pClusterManager) {
    this->name = std::move(name);
    this->pClusterManager = pClusterManager;
}

void Cluster::connect(std::string token) {
    std::cout << "Attempting to connect cluster " << name << " with token " << token << std::endl;
    std::system(("cd ./utils/keyserver; ./keyserver " + name + " " + token).c_str());
}

void Cluster::setConnection(WsServer::Connection *pConnection) {
    this->pConnection = pConnection;
}

void Cluster::queueMessage(std::vector<uint8_t> *data, Message::Priority priority) {

}

//     auto o = std::make_shared<WsServer::OutMessage>(data.size());
//    std::ostream_iterator<uint8_t> iter(*o.get());
//    std::copy(data.begin(), data.end(), iter);