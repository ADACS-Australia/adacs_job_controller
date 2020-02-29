//
// Created by lewis on 2/27/20.
//

#include "MessageScheduler.h"

MessageScheduler::MessageScheduler(unsigned int maxPriority) {
    std::thread schedulerThread([this] {
        this->run();
    });
}

void MessageScheduler::run() {
    while (true) {
        // Wait for data to be ready to send
        std::unique_lock<std::mutex> lk(mutex_);
    }
}
