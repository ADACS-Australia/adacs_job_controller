//
// WebSocketServer C++20 module
//

module;
// Hack to prevent DEPRECATED from being undefined in server_ws.hpp
#ifndef DEPRECATED
#define DEPRECATED
#endif
#include <iostream>
#include <memory>
#include <thread>
#include <utility>

#include <server_ws.hpp>

export module WebSocketServer;

import settings;
import Message;
import IClusterManager;
import IApplication;
import GeneralUtils;
import IWebSocketServer;

export using WsServer = SimpleWeb::SocketServer<SimpleWeb::WS>;

export class WebSocketServer : public IWebSocketServer
{
public:
    explicit WebSocketServer(std::shared_ptr<IApplication> app);

    void start() override;
    void join() override;
    void stop() override;
    bool is_running() const override;

private:
    WsServer server;
    std::thread server_thread;

    std::shared_ptr<IApplication> app;
};

// Implementation
WebSocketServer::WebSocketServer(std::shared_ptr<IApplication> app) : app(std::move(app))
{
    server.config.port             = WEBSOCKET_PORT;
    server.config.address          = "0.0.0.0";
    server.config.thread_pool_size = WEBSOCKET_WORKER_POOL_SIZE;

    auto& wsEp = server.endpoint["^/job/ws/$"];

    wsEp.on_message = [this](const std::shared_ptr<WsServer::Connection>& connection,
                             const std::shared_ptr<WsServer::InMessage>& in_message) {
        // Try to get the cluster from the connection
        auto cluster = this->app->getClusterManager()->getCluster(connection);
        if (!cluster)
        {
            // What?
            connection->close();
            return;
        }

        // Convert the string to a vector and create the message object
        auto msg = Message(in_message->data());
        cluster->handleMessage(msg);
    };

    wsEp.on_open = [this](const std::shared_ptr<WsServer::Connection>& connection) {
        // Parse the query string so we can obtain the token
        auto queryParams = SimpleWeb::QueryString::parse(connection->query_string);

        // There should only be one query string parameter
        if (queryParams.size() != 1)
        {
            connection->close();
            return;
        }

        // Check that the only query string parameter is "token"
        if ((*queryParams.begin()).first != "token")
        {
            connection->close();
            return;
        }

        // Check that the token is valid
        auto cluster = this->app->getClusterManager()->handleNewConnection(connection, (*queryParams.begin()).second);
        if (cluster)
        {
            // Everything is fine
            std::cout << "WS: Opened connection from " << cluster->getName() << " as role " << cluster->getRoleString()
                      << '\n';

            // Tell the client that we are ready
            Message msg(SERVER_READY, Message::Priority::Highest, SYSTEM_SOURCE);
            cluster->sendMessage(msg);
        }
        else
        {
            // Invalid Token
            std::cout << "WS: Invalid token used - " << (*queryParams.begin()).second << '\n';
            connection->close();
        }
    };

    wsEp.on_close =
        [this](const std::shared_ptr<WsServer::Connection>& connection, int status, const std::string& /*reason*/) {
            // Try to get the cluster from the connection
            auto cluster = this->app->getClusterManager()->getCluster(connection);

            // Remove the cluster from the connected list
            if (cluster)
            {
                this->app->getClusterManager()->removeConnection(connection, true, true);
            }  // Log this
            std::cout << "WS: Closed connection with " << std::string(cluster ? cluster->getName() : "unknown?")
                      << " with status code " << status << '\n';
        };

    wsEp.on_error = [this](const std::shared_ptr<WsServer::Connection>& connection,
                           const SimpleWeb::error_code& errorCode) {
        // Try to get the cluster from the connection
        auto cluster = this->app->getClusterManager()->getCluster(connection);

        // Remove the cluster from the connected list
        if (cluster)
        {
            this->app->getClusterManager()->removeConnection(connection, true, true);
        }

        this->app->getClusterManager()->reportWebsocketError(cluster, errorCode);
    };

    wsEp.on_pong = [this](const std::shared_ptr<WsServer::Connection>& connection) {
        // Get the cluster associated with this connection
        auto cluster = this->app->getClusterManager()->getCluster(connection);
        if (cluster)
        {
            this->app->getClusterManager()->handlePong(connection);
        }
    };
}

void WebSocketServer::start()
{
    server_thread = std::thread([this]() {
        server.start();
    });
}

void WebSocketServer::join()
{
    server_thread.join();
}

void WebSocketServer::stop()
{
    server.stop();
    if (server_thread.joinable())
    {
        server_thread.join();
    }
}

bool WebSocketServer::is_running() const
{
    return server_thread.joinable();
}
