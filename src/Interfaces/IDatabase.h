//
// Interface for database operations
// This breaks circular dependencies by providing a pure interface
//

#ifndef GWCLOUD_JOB_SERVER_I_DATABASE_H
#define GWCLOUD_JOB_SERVER_I_DATABASE_H

#include <cstdint>
#include <memory>
#include <string>
#include <vector>

// Forward declarations to avoid circular dependencies
class Message;
class ICluster;

// Interface for database operations
class IDatabase
{
public:
    virtual ~IDatabase() = default;

    // Database message handling
    virtual auto canHandleDatabaseMessage(const Message& message) const -> bool                    = 0;
    virtual void handleDatabaseMessage(Message& message, const std::shared_ptr<ICluster>& cluster) = 0;

    // Cluster job operations
    virtual auto getClusterJobByJobId(uint64_t jobId, const std::string& cluster) const -> std::vector<uint8_t>    = 0;
    virtual auto getClusterJobById(uint64_t id, const std::string& cluster) const -> std::vector<uint8_t>          = 0;
    virtual auto getRunningClusterJobs(const std::string& cluster) const -> std::vector<uint8_t>                   = 0;
    virtual auto saveClusterJob(const std::vector<uint8_t>& jobData, const std::string& cluster) const -> uint64_t = 0;
    virtual void deleteClusterJob(uint64_t id, const std::string& cluster)                                         = 0;

    // Cluster job status operations
    virtual auto getClusterJobStatusByJobId(uint64_t jobId,
                                            const std::string& cluster) const -> std::vector<uint8_t>         = 0;
    virtual auto getClusterJobStatusByJobIdAndWhat(uint64_t jobId,
                                                   const std::string& what,
                                                   const std::string& cluster) const -> std::vector<uint8_t>  = 0;
    virtual auto saveClusterJobStatus(const std::vector<uint8_t>& statusData,
                                      const std::string& cluster) const -> uint64_t                           = 0;
    virtual void deleteClusterJobStatusByIdList(const std::vector<uint64_t>& ids, const std::string& cluster) = 0;

    // Bundle job operations
    virtual auto createOrUpdateBundleJob(const std::vector<uint8_t>& jobData,
                                         const std::string& cluster,
                                         const std::string& bundleHash) const -> uint64_t                    = 0;
    virtual auto getBundleJobById(uint64_t id,
                                  const std::string& cluster,
                                  const std::string& bundleHash) const -> std::vector<uint8_t>               = 0;
    virtual void deleteBundleJobById(uint64_t id, const std::string& cluster, const std::string& bundleHash) = 0;
};

#endif  // GWCLOUD_JOB_SERVER_I_DATABASE_H
