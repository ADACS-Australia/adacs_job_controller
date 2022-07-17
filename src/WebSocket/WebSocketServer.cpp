//
// Created by lewis on 2/26/20.
//

#include "WebSocketServer.h"
#include "../Cluster/ClusterManager.h"

using namespace std;

WebSocketServer::WebSocketServer(std::shared_ptr<ClusterManager> clusterManager) {
    this->clusterManager = clusterManager;

    server.config.port = 8001;
    server.config.address = "0.0.0.0";
    server.config.thread_pool_size = 32;

    auto &wsEp = server.endpoint["^/job/ws/$"];

    wsEp.on_message = [this](const shared_ptr<WsServer::Connection>& connection, const shared_ptr<WsServer::InMessage>& in_message) {
        // Try to get the cluster from the connection
        auto cluster = this->clusterManager->getCluster(connection);
        if (!cluster) {
            // What?
            connection->send_close(1000, "Bye.");
            return;
        }

        // Convert the string to a vector and create the message object
        auto m = Message(in_message->data());

        // Handle the message
        cluster->handleMessage(m);
    };

    wsEp.on_open = [this](const shared_ptr<WsServer::Connection>& connection) {
        // Parse the query string so we can obtain the token
        auto qp = SimpleWeb::QueryString::parse(connection->query_string);

        // There should only be one query string parameter
        if (qp.size() != 1) {
            connection->send_close(1000, "Bye.");
            return;
        }

        // Check that the only query string parameter is "token"
        if ((*qp.begin()).first != "token") {
            connection->send_close(1000, "Bye.");
            return;
        }

        // Check that the token is valid
        auto cluster = this->clusterManager->handleNewConnection(connection, (*qp.begin()).second);
        if (cluster) {
            // Everything is fine
            cout << "WS: Opened connection from " << cluster->getName() << endl;

            // Tell the client that we are ready
            Message msg(SERVER_READY, Message::Priority::Highest, SYSTEM_SOURCE);
            msg.send(cluster);
        } else {
            // Invalid Token
            cout << "WS: Invalid token used - " << (*qp.begin()).second << endl;
            connection->send_close(1000, "Bye.");
        }
    };

    // See RFC 6455 7.4.1. for status codes
    wsEp.on_close = [this](const shared_ptr<WsServer::Connection>& connection, int status, const string & /*reason*/) {
        // Try to get the cluster from the connection
        auto cluster = this->clusterManager->getCluster(connection);

        // Remove the cluster from the connected list
        if (cluster)
            this->clusterManager->removeConnection(connection);

        // Log this
        cout << "WS: Closed connection with " << std::string(cluster ? cluster->getName() : "unknown?") << " with status code " << status << endl;
    };

    // See http://www.boost.org/doc/libs/1_55_0/doc/html/boost_asio/reference.html, Error Codes for error code meanings
    wsEp.on_error = [this](const shared_ptr<WsServer::Connection>& connection, const SimpleWeb::error_code &ec) {
        // Try to get the cluster from the connection
        auto cluster = this->clusterManager->getCluster(connection);

        // Remove the cluster from the connected list
        if (cluster)
            this->clusterManager->removeConnection(connection);

        // Log this
        cout << "WS: Error in connection with " << std::string(cluster ? cluster->getName() : "unknown?") << ". "
             << "Error: " << ec << ", error message: " << ec.message() << endl;
    };

}

bool WebSocketServer::accepting_connections(unsigned short port) {
    using namespace boost::asio;
    using ip::tcp;
    using ec = boost::system::error_code;

    bool result = false;

    try
    {
        io_service svc;
        tcp::socket s(svc);
        deadline_timer tim(svc, boost::posix_time::milliseconds (100));

        tim.async_wait([&](ec) { s.cancel(); });
        s.async_connect({{}, port}, [&](ec ec) {
            result = !ec;
        });

        svc.run();
    } catch(...) { }

    return result;
}

void WebSocketServer::start() {
    server_thread = thread([this]() {
        // Start server
        this->server.start();
    });

    // Wait a for the server to initialise
    while (!accepting_connections(server.config.port));

    cout << "WS: Server listening on port " << server.config.port << endl;
}

void WebSocketServer::join() {
    server_thread.join();
}

void WebSocketServer::stop() {
    server.stop();
    join();
}