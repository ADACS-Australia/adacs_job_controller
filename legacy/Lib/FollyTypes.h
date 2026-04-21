//
// Folly types header - includes folly headers once to avoid module conflicts
//

#ifndef GWCLOUD_JOB_SERVER_FOLLY_TYPES_H
#define GWCLOUD_JOB_SERVER_FOLLY_TYPES_H

#include <folly/concurrency/ConcurrentHashMap.h>
#include <folly/concurrency/UnboundedQueue.h>

#include "FileTypes.h"

// Type aliases for commonly used folly types
using FileListMap = folly::ConcurrentHashMap<std::string, std::shared_ptr<sFileList>>;

#endif  // GWCLOUD_JOB_SERVER_FOLLY_TYPES_H
