#include <execinfo.h>
#include "HTTP/HttpServer.h"
#include "WebSocket/WebSocketServer.h"
#include "Cluster/ClusterManager.h"
#include "Lib/segvcatch.h"

using namespace segvcatch;

void handle_segv()
{
    void *array[10];
    size_t size;

    // get void*'s for all entries on the stack
    size = backtrace(array, 10);

    // print out all the frames to stderr
    fprintf(stderr, "Error: SEGFAULT:\n");
    backtrace_symbols_fd(array, size, STDERR_FILENO);

    throw std::runtime_error("Seg Fault Error");
}

int main() {
    // Set up the crash handler
    segvcatch::init_segv(&handle_segv);

    auto clusterManager = new ClusterManager();
    auto httpServer = new HttpServer(clusterManager);
    auto websocketServer = new WebSocketServer(clusterManager);

    // Start the websocket server
    websocketServer->start();

    // Now that the websocket is listening, start the cluster manager
    clusterManager->start();

    // Now finally start the http server to handle api requests
    httpServer->start();

    return 0;
}
