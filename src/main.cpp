#include "Cluster/ClusterManager.h"
#include "HTTP/HttpServer.h"
#include "Lib/segvcatch.h"
#include "WebSocket/WebSocketServer.h"

#include <folly/experimental/exception_tracer/StackTrace.h>

auto main() -> int
{
    // Set up the crash handler
    segvcatch::init_segv(&handleSegv);

    auto clusterManager = std::make_shared<ClusterManager>();
    auto httpServer = std::make_unique<HttpServer>(clusterManager);
    auto websocketServer = std::make_unique<WebSocketServer>(clusterManager);

    // Start the websocket server
    websocketServer->start();

    // Now that the websocket is listening, start the cluster manager
    clusterManager->start();

    // Now finally start the http server to handle api requests
    httpServer->start();
    httpServer->join();

    return 0;
}

// To prevent the compiler optimizing away the exception tracing from folly, we need to reference it.
extern "C" auto getCaughtExceptionStackTraceStack() -> const folly::exception_tracer::StackTrace*;
extern "C" auto getUncaughtExceptionStackTraceStack() -> const folly::exception_tracer::StackTraceStack*;

// forceExceptionStackTraceRef is intentionally unused and marked volatile so the compiler doesn't optimize away the
// required functions from folly. This is black magic.
volatile void forceExceptionStackTraceRef()
{
    getCaughtExceptionStackTraceStack();
    getUncaughtExceptionStackTraceStack();
}
