//
// Interface for HTTP service functionality
// This breaks circular dependencies by providing a pure interface
//

#ifndef GWCLOUD_JOB_SERVER_I_HTTP_SERVICE_H
#define GWCLOUD_JOB_SERVER_I_HTTP_SERVICE_H

#include <memory>
#include <string>
#include <cstdint>

// Forward declarations to avoid circular dependencies
class ICluster;
class Message;

// Interface for HTTP service operations
class IHttpService {
public:
    virtual ~IHttpService() = default;
    
    // Job management
    virtual void submitJob(const std::string& jobData, const std::shared_ptr<ICluster>& cluster) = 0;
    virtual void updateJob(const std::string& jobId, const std::string& jobData, const std::shared_ptr<ICluster>& cluster) = 0;
    virtual void cancelJob(const std::string& jobId, const std::shared_ptr<ICluster>& cluster) = 0;
    virtual void deleteJob(const std::string& jobId, const std::shared_ptr<ICluster>& cluster) = 0;
    
    // File operations
    virtual void downloadFile(const std::string& filePath, const std::shared_ptr<ICluster>& cluster) = 0;
    virtual void getFileDetails(const std::string& filePath, const std::shared_ptr<ICluster>& cluster) = 0;
    virtual void getFileList(const std::string& directory, const std::shared_ptr<ICluster>& cluster) = 0;
    
    // Message handling
    virtual void handleMessage(Message& message, const std::shared_ptr<ICluster>& cluster) = 0;
    
    // Server lifecycle
    virtual void start() = 0;
    virtual void stop() = 0;
};

#endif //GWCLOUD_JOB_SERVER_I_HTTP_SERVICE_H
