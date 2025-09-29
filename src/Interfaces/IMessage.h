//
// Interface for message functionality
// This breaks circular dependencies by providing a pure interface
//

#ifndef GWCLOUD_JOB_SERVER_I_MESSAGE_H
#define GWCLOUD_JOB_SERVER_I_MESSAGE_H

#include <cstdint>
#include <memory>
#include <string>
#include <vector>

// Forward declaration to avoid circular dependencies
class ICluster;

// Interface for message operations
class IMessage
{
public:
    virtual ~IMessage() = default;

    // Message identification
    virtual auto getId() const -> uint32_t               = 0;
    virtual auto getSource() const -> const std::string& = 0;
    virtual auto getPriority() const -> uint32_t         = 0;

    // Data access
    virtual auto getData() const -> const std::vector<uint8_t>& = 0;
    virtual void setData(const std::vector<uint8_t>& data)      = 0;

    // Message sending (now takes ICluster instead of concrete Cluster)
    virtual void send(const std::shared_ptr<ICluster>& cluster) = 0;

    // Data manipulation
    virtual void push_ulong(uint64_t value)            = 0;
    virtual void push_uint(uint32_t value)             = 0;
    virtual void push_string(const std::string& value) = 0;
    virtual void push_bool(bool value)                 = 0;

    virtual auto pop_ulong() -> uint64_t     = 0;
    virtual auto pop_uint() -> uint32_t      = 0;
    virtual auto pop_string() -> std::string = 0;
    virtual auto pop_bool() -> bool          = 0;
};

#endif  // GWCLOUD_JOB_SERVER_I_MESSAGE_H
