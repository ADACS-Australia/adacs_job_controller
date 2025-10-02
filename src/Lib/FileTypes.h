//
// File type definitions
// This breaks circular dependencies by providing shared types
//

#ifndef GWCLOUD_JOB_SERVER_FILE_TYPES_H
#define GWCLOUD_JOB_SERVER_FILE_TYPES_H

#include <condition_variable>
#include <mutex>
#include <string>
#include <vector>

struct sFile
{
    std::string fileName;
    uint64_t fileSize    = 0;
    uint32_t permissions = 0;
    bool isDirectory     = false;
};

struct sFileList
{
    std::vector<sFile> files;
    bool error = false;
    std::string errorDetails;
    mutable std::mutex dataCVMutex;
    bool dataReady{};
    std::condition_variable dataCV;
};

#endif  // GWCLOUD_JOB_SERVER_FILE_TYPES_H
