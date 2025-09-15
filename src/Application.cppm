//
// Application implementation module
// Contains all the global state that was previously in GlobalState
//

module;
#include <iostream>
#include <memory>
#include <mutex>
#include <string>
#include <vector>
#include "Lib/FollyTypes.h"
#include "Lib/FileTypes.h"

export module Application;

import IApplication;
import IHttpServer;
import HttpServer;
import WebSocketServer;
import IWebSocketServer;
import ClusterManager;
import IClusterManager;
import Job;
import File;

class Application : public IApplication, public std::enable_shared_from_this<Application> {
private:
    std::shared_ptr<FileListMap> fileListMap;
    std::mutex fileDownloadPauseResumeLockMutex;
    std::mutex fileListMapDeletionLockMutex;
    bool running;
    
    // Server components
    std::shared_ptr<ClusterManager> clusterManager;
    std::shared_ptr<IHttpServer> httpServer;
    std::shared_ptr<IWebSocketServer> websocketServer;

public:
    Application() : running(false) {
        fileListMap = std::make_shared<FileListMap>();
    }

    void initializeComponents() {
        // Initialize server components with this application reference
        clusterManager = std::make_shared<ClusterManager>(shared_from_this());
        // Create concrete HttpServer but store as interface
        auto concreteHttpServer = std::make_shared<HttpServer>(shared_from_this());
        httpServer = concreteHttpServer;
        // Create concrete WebSocketServer but store as interface
        auto concreteWebSocketServer = std::make_shared<WebSocketServer>(shared_from_this());
        websocketServer = concreteWebSocketServer;

        JobApi("/job/apiv1/job/", concreteHttpServer, shared_from_this());
        FileApi("/job/apiv1/file/", concreteHttpServer, shared_from_this());
    }

    ~Application() override = default;

    // IApplication interface implementation
    std::shared_ptr<FileListMap> getFileListMap() override {
        return fileListMap;
    }

    std::mutex& getFileDownloadPauseResumeLockMutex() override {
        return fileDownloadPauseResumeLockMutex;
    }

    std::mutex& getFileListMapDeletionLockMutex() override {
        return fileListMapDeletionLockMutex;
    }

    void initialize() override {
        std::cout << "Application initializing..." << std::endl;
        running = true;
    }

    void shutdown() override {
        std::cout << "Application shutting down..." << std::endl;
        running = false;
    }

    bool isRunning() const override {
        return running;
    }
    
    // Server component access
    std::shared_ptr<IClusterManager> getClusterManager() override {
        return clusterManager;
    }

    std::shared_ptr<IHttpServer> getHttpServer() override {
        return httpServer;
    }

    std::shared_ptr<IWebSocketServer> getWebSocketServer() override {
        return websocketServer;
    }
    
    // Main application run method
    void run() {
        initialize();
        
        // Start the websocket server
        websocketServer->start();

        // Now that the websocket is listening, start the cluster manager
        clusterManager->start();

        // Now finally start the http server to handle api requests
        httpServer->start();
        httpServer->join();
    }
};

// Factory function to create application instance
export std::shared_ptr<IApplication> createApplication() {
    auto app = std::make_shared<Application>();
    app->initializeComponents();
    return app;
}
