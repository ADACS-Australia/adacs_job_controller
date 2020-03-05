//
// Created by lewis on 2/26/20.
//

#include "WebSocketServer.h"
#include "../Cluster/ClusterManager.h"
#include "../Lib/Messaging/Message.h"

using namespace std;

WebSocketServer::WebSocketServer(ClusterManager* clusterManager) {
    this->clusterManager = clusterManager;

    server.config.port = 8001;

    auto &echo = server.endpoint["^/ws/?$"];

    echo.on_message = [this](const shared_ptr<WsServer::Connection>& connection, const shared_ptr<WsServer::InMessage>& in_message) {
        // Try to get the cluster from the connection
        auto cluster = this->clusterManager->get_cluster(connection.get());
        if (!cluster) {
            // What?
            connection->send_close(1000, "Bye.");
            return;
        }

        // Get the string representation of the message
        auto s = in_message->string();

        // Convert the string to a vector and create the message object
        auto m = Message(vector<uint8_t>(s.begin(), s.end()));

        // Handle the message
        cluster->handleMessage(m);
    };

    echo.on_open = [this](const shared_ptr<WsServer::Connection>& connection) {
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
        auto cluster = this->clusterManager->handle_new_connection(connection.get(), (*qp.begin()).second);
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
    echo.on_close = [this](const shared_ptr<WsServer::Connection>& connection, int status, const string & /*reason*/) {
        // Try to get the cluster from the connection
        auto cluster = this->clusterManager->get_cluster(connection.get());

        // Remove the cluster from the connected list
        this->clusterManager->remove_connection(connection.get());

        // Log this
        cout << "WS: Closed connection with " << std::string(cluster ? cluster->getName() : "unknown?") << " with status code " << status << endl;
    };

    // See http://www.boost.org/doc/libs/1_55_0/doc/html/boost_asio/reference.html, Error Codes for error code meanings
    echo.on_error = [this](const shared_ptr<WsServer::Connection>& connection, const SimpleWeb::error_code &ec) {
        // Try to get the cluster from the connection
        auto cluster = this->clusterManager->get_cluster(connection.get());

        // Remove the cluster from the connected list
        this->clusterManager->remove_connection(connection.get());

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
    auto server_ptr = &server;
    server_thread = thread([&server_ptr]() {
        // Start server
        server_ptr->start();
    });

    // Wait a for the server to initialise
    while (!accepting_connections(server.config.port));

    cout << "WS: Server listening on port " << server.config.port << endl;
}
