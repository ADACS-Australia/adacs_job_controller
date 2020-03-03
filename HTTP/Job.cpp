//
// Created by lewis on 3/4/20.
//

#include "HttpServer.h"
using namespace std;

void JobApi(std::string path, HttpServerImpl* server) {
    // Get      -> Get job status (job id)
    // Post     -> Create new job
    // Delete   -> Delete job (job id)
    // Patch    -> Cancel job (job id)

    // Create a new job
    server->resource["^" + path + "$"]["POST"] = [](shared_ptr<HttpServerImpl::Response> response, shared_ptr<HttpServerImpl::Request> request) {
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

    server->resource["^/info$"]["GET"] = [](shared_ptr<HttpServerImpl::Response> response, shared_ptr<HttpServerImpl::Request> request) {
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