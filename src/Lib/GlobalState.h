//
// Global state declarations
// This breaks circular dependencies by providing shared global state
//

#ifndef GWCLOUD_JOB_SERVER_GLOBAL_STATE_H
#define GWCLOUD_JOB_SERVER_GLOBAL_STATE_H

#include "FileTypes.h"
#include <folly/concurrency/ConcurrentHashMap.h>
#include <mutex>

extern const std::shared_ptr<folly::ConcurrentHashMap<std::string, std::shared_ptr<sFileList>>> fileListMap;
// NOLINTBEGIN(cppcoreguidelines-avoid-non-const-global-variables)
extern std::mutex fileDownloadPauseResumeLockMutex;
extern std::mutex fileListMapDeletionLockMutex;
// NOLINTEND(cppcoreguidelines-avoid-non-const-global-variables)

#endif //GWCLOUD_JOB_SERVER_GLOBAL_STATE_H
