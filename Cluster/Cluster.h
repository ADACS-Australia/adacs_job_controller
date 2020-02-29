//
// Created by lewis on 2/27/20.
//

#ifndef GWCLOUD_JOB_SERVER_CLUSTER_H
#define GWCLOUD_JOB_SERVER_CLUSTER_H


#include <string>
#include <boost/uuid/uuid.hpp>
#include "../WebSocket/WebSocketServer.h"
#include "../Lib/Messaging/Message.h"
#include <boost/lockfree/queue.hpp>

class ClusterManager;
class MessageScheduler;

class Cluster {
public:
    Cluster(std::string name, ClusterManager* pClusterManager);

    void connect(std::string token);

    std::string getName() { return name; }

    void setConnection(WsServer::Connection *pConnection);

    void queueMessage(std::string source, std::vector<uint8_t>* data, Message::Priority priority);

private:
    void run();

    std::string name;
    WsServer::Connection* pConnection = nullptr;
    ClusterManager* pClusterManager = nullptr;

    mutable std::mutex mutex_;
    std::condition_variable conditionVariable;
    std::list<std::map<std::string, boost::lockfree::queue<std::vector<uint8_t>*>>> queue;
    std::thread schedulerThread;
};


#endif //GWCLOUD_JOB_SERVER_CLUSTER_H
