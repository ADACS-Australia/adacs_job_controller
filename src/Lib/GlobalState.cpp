//
// Global state definitions
// This centralizes all global state to avoid circular dependencies
//

#include "GlobalState.h"

// Global state definitions
const std::shared_ptr<folly::ConcurrentHashMap<std::string, std::shared_ptr<sFileList>>> fileListMap = 
    std::make_shared<folly::ConcurrentHashMap<std::string, std::shared_ptr<sFileList>>>();

// NOLINTBEGIN(cppcoreguidelines-avoid-non-const-global-variables)
std::mutex fileDownloadPauseResumeLockMutex;
std::mutex fileListMapDeletionLockMutex;
// NOLINTEND(cppcoreguidelines-avoid-non-const-global-variables)
