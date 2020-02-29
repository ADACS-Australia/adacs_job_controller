//
// Created by lewis on 2/27/20.
//

#ifndef GWCLOUD_JOB_SERVER_MESSAGESCHEDULER_H
#define GWCLOUD_JOB_SERVER_MESSAGESCHEDULER_H

#include <mutex>
#include <shared_mutex>
#include <thread>
#include <list>
#include <map>
#include <vector>
#include <boost/lockfree/queue.hpp>

class MessageScheduler {
    // Queue is a:
    //  list of priorities - doesn't need any sync because it never changes
    //      -> map of sources - needs sync when adding/removing sources
    //          -> vector of packets - make this a MPSC queue

    // When the number of bytes in a packet of vectors exceeds some amount, a message should be sent that stops more
    // packets from being sent, when the vector then falls under some threshold

    // Track sources in the map, add them when required - delete them after some amount (1 minute?) of inactivity.

    // Send sources round robin, starting from the highest priority
public:
    MessageScheduler(unsigned int maxPriority);

    void run();

private:
    mutable std::mutex mutex_;
    std::condition_variable conditionVariable;
    std::list<std::map<std::string, boost::lockfree::queue<std::vector<uint8_t>*>>> queue;
    std::thread schedulerThread;
};


#endif //GWCLOUD_JOB_SERVER_MESSAGESCHEDULER_H
