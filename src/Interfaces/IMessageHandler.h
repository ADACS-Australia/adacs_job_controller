//
// Interface for message handling functionality
// This breaks circular dependencies by providing a pure interface
//

#ifndef GWCLOUD_JOB_SERVER_I_MESSAGE_HANDLER_H
#define GWCLOUD_JOB_SERVER_I_MESSAGE_HANDLER_H

#include <cstdint>
#include <memory>
#include <string>
#include <vector>

// Forward declarations to avoid circular dependencies
class Message;
class ICluster;

// Interface for message handling operations
class IMessageHandler
{
public:
    virtual ~IMessageHandler() = default;

    // Message processing
    virtual auto canHandleMessage(const Message& message) const -> bool                    = 0;
    virtual void handleMessage(Message& message, const std::shared_ptr<ICluster>& cluster) = 0;

    // Message creation helpers
    virtual auto createResponseMessage(const Message& originalMessage,
                                       const std::string& source) const -> std::unique_ptr<Message> = 0;
};

#endif  // GWCLOUD_JOB_SERVER_I_MESSAGE_HANDLER_H
