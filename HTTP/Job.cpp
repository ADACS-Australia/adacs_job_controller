//
// Created by lewis on 3/4/20.
//

#include "HttpServer.h"
#include "../DB/MySqlConnector.h"
#include "../Lib/jobserver_schema.h"
#include "../Cluster/ClusterManager.h"

using namespace std;
using namespace schema;

void JobApi(std::string path, HttpServerImpl* server, ClusterManager* clusterManager) {
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

    server->resource["^/info$"]["GET"] = [clusterManager](shared_ptr<HttpServerImpl::Response> response, shared_ptr<HttpServerImpl::Request> request) {

//        auto db = MySqlConnector();
//        JobserverJob tab;
//
//        db->run(insert_into(tab).set(tab.parameters = "Even more params!", tab.state = 5));
//
//        for(const auto& row : db->run(sqlpp::select(all_of(tab)).from(tab).unconditionally()))
//        {
//            std::cerr << "row.id: " << row.id << ", row.parameters: " << row.parameters << std::endl;
//        };

        auto cluster = clusterManager->getCluster("ozstar");
        auto msg = Message(SUBMIT_JOB, Message::Priority::Medium, "system");
        msg.push_uint(5);
        msg.push_string("be725c18ea8cd4307ec787d30f684ea5064c960a1cdaa1b100f9fe024d7d033f");
        msg.push_string("some job data");
        msg.send(cluster);

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