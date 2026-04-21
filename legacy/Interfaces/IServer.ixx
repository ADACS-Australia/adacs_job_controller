//
// Common server interface
// Defines the basic lifecycle methods that all servers should implement
//

export module IServer;

export class IServer
{
protected:
    IServer() = default;

public:
    virtual ~IServer() = default;

    // Prevent copying and moving
    IServer(const IServer&)            = delete;
    IServer& operator=(const IServer&) = delete;
    IServer(IServer&&)                 = delete;
    IServer& operator=(IServer&&)      = delete;

    // Server lifecycle methods
    virtual void start()                          = 0;
    virtual void stop()                           = 0;
    virtual void join()                           = 0;
    [[nodiscard]] virtual bool is_running() const = 0;
};
