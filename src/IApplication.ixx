//
// Application interface module
// Provides access to application state without global variables
//

module;

#include <memory>
#include <mutex>
#include <string>
#include <vector>

#include "Lib/FollyTypes.h"

export module IApplication;

// Forward declarations for server classes
class IClusterManager;
class IHttpServer;
class IWebSocketServer;

// Re-export the FileListMap type
export using FileListMap = ::FileListMap;

export class IApplication
{
public:
    virtual ~IApplication() = default;

    // File list management
    virtual std::shared_ptr<FileListMap> getFileListMap()     = 0;
    virtual std::mutex& getFileDownloadPauseResumeLockMutex() = 0;
    virtual std::mutex& getFileListMapDeletionLockMutex()     = 0;

    // Server component access
    virtual std::shared_ptr<IClusterManager> getClusterManager()   = 0;
    virtual std::shared_ptr<IHttpServer> getHttpServer()           = 0;
    virtual std::shared_ptr<IWebSocketServer> getWebSocketServer() = 0;

    // Application lifecycle
    virtual void initialize()      = 0;
    virtual void shutdown()        = 0;
    virtual bool isRunning() const = 0;
    virtual void run()             = 0;
};
