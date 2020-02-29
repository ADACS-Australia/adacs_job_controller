//
// Created by lewis on 2/26/20.
//

#include "HttpServer.h"
using namespace std;

HttpServer::HttpServer() {
    {
        server.config.port = 8000;

        // Add resources using path-regex and method-string, and an anonymous function
        // POST-example for the path /string, responds the posted string
        server.resource["^/string$"]["POST"] = [](shared_ptr<HttpServerImpl::Response> response, shared_ptr<HttpServerImpl::Request> request) {
            // Retrieve string:
            auto content = request->content.string();
            // request->content.string() is a convenience function for:
            // stringstream ss;
            // ss << request->content.rdbuf();
            // auto content=ss.str();

            *response << "HTTP/1.1 200 OK\r\nContent-Length: " << content.length() << "\r\n\r\n"
                      << content;


            // Alternatively, use one of the convenience functions, for instance:
            // response->write(content);
        };

        server.resource["^/info$"]["GET"] = [](shared_ptr<HttpServerImpl::Response> response, shared_ptr<HttpServerImpl::Request> request) {
            stringstream stream;
            stream << "<h1>Request from " << request->remote_endpoint_address() << ":" << request->remote_endpoint_port() << "</h1>";

            stream << request->method << " " << request->path << " HTTP/" << request->http_version;

            stream << "<h2>Query Fields</h2>";
            auto query_fields = request->parse_query_string();
            for(auto &field : query_fields)
                stream << field.first << ": " << field.second << "<br>";

            stream << "<h2>Header Fields</h2>";
            for(auto &field : request->header)
                stream << field.first << ": " << field.second << "<br>";

            response->write(stream);
        };
    }
}

void HttpServer::start() {
    auto server_ptr = &server;
    server_thread = thread([&server_ptr]() {
        // Start server
        server_ptr->start();
    });

    cout << "Http server listening on port " << server.config.port << endl << endl;

    server_thread.join();
}
