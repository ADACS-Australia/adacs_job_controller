//
// Common server interface
// Defines the basic lifecycle methods that all servers should implement
//

export module IServer;

export class IServer
{
public:
    virtual ~IServer() = default;

    // Server lifecycle methods
    virtual void start()            = 0;
    virtual void stop()             = 0;
    virtual void join()             = 0;
    virtual bool is_running() const = 0;
};
