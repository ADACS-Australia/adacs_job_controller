//
// Created by lewis on 2/26/20.
//

#include "../Cluster/ClusterManager.h"
#include "../Settings.h"
#include "WebSocketServer.h"

#include <utility>

WebSocketServer::WebSocketServer(std::shared_ptr<ClusterManager> clusterManager) : clusterManager(std::move(clusterManager)) {
    server.config.port = WEBSOCKET_PORT;
    server.config.address = "0.0.0.0";
    server.config.thread_pool_size = WEBSOCKET_WORKER_POOL_SIZE;

    auto &wsEp = server.endpoint["^/job/ws/$"];

    wsEp.on_message = [this](const std::shared_ptr<WsServer::Connection>& connection, const std::shared_ptr<WsServer::InMessage>& in_message) {
        // Try to get the cluster from the connection
        auto cluster = this->clusterManager->getCluster(connection);
        if (!cluster) {
            // What?
            // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
            connection->send_close(1000, "Bye.");
            return;
        }

        // Convert the string to a vector and create the message object
        auto message = Message(in_message->data());

        // Handle the message
        cluster->handleMessage(message);
    };

    wsEp.on_open = [this](const std::shared_ptr<WsServer::Connection>& connection) {
        // Parse the query string so we can obtain the token
        auto queryParams = SimpleWeb::QueryString::parse(connection->query_string);

        // There should only be one query string parameter
        if (queryParams.size() != 1) {
            // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
            connection->send_close(1000, "Bye.");
            return;
        }

        // Check that the only query string parameter is "token"
        if ((*queryParams.begin()).first != "token") {
            // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
            connection->send_close(1000, "Bye.");
            return;
        }

        // Check that the token is valid
        auto cluster = this->clusterManager->handleNewConnection(connection, (*queryParams.begin()).second);
        if (cluster) {
            // Everything is fine
            std::cout << "WS: Opened connection from " << cluster->getName() << std::endl;

            // Tell the client that we are ready
            Message msg(SERVER_READY, Message::Priority::Highest, SYSTEM_SOURCE);
            msg.send(cluster);
        } else {
            // Invalid Token
            std::cout << "WS: Invalid token used - " << (*queryParams.begin()).second << std::endl;
            // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
            connection->send_close(1000, "Bye.");
        }
    };

    // See RFC 6455 7.4.1. for status codes
    wsEp.on_close = [this](const std::shared_ptr<WsServer::Connection>& connection, int status, const std::string & /*reason*/) {
        // Try to get the cluster from the connection
        auto cluster = this->clusterManager->getCluster(connection);

        // Remove the cluster from the connected list
        if (cluster) {
            this->clusterManager->removeConnection(connection);
        }

        // Log this
        std::cout << "WS: Closed connection with " << std::string(cluster ? cluster->getName() : "unknown?") << " with status code " << status << std::endl;
    };

    // See http://www.boost.org/doc/libs/1_55_0/doc/html/boost_asio/reference.html, Error Codes for error code meanings
    wsEp.on_error = [this](const std::shared_ptr<WsServer::Connection>& connection, const SimpleWeb::error_code &errorCode) {
        // Try to get the cluster from the connection
        auto cluster = this->clusterManager->getCluster(connection);

        // Remove the cluster from the connected list
        if (cluster) {
            this->clusterManager->removeConnection(connection);
        }

        // Log this
        std::cout << "WS: Error in connection with " << std::string(cluster ? cluster->getName() : "unknown?") << ". "
                  << "Error: " << errorCode << ", error message: " << errorCode.message() << std::endl;
    };

}

void WebSocketServer::start() {
    server_thread = std::thread([this]() {
        // Start server
        this->server.start();
    });

    // Wait a for the server to initialise
    while (!acceptingConnections(server.config.port)){ }

    std::cout << "WS: Server listening on port " << server.config.port << std::endl;
}

void WebSocketServer::join() {
    server_thread.join();
}

void WebSocketServer::stop() {
    server.stop();
    join();
}