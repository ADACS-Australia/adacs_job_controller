//
// Created by lewis on 3/11/20.
//

#include "HttpUtils.h"
#include "../Lib/JobStatus.h"
#include "../Cluster/ClusterManager.h"
#include "../Lib/jobserver_schema.h"
#include "../DB/MySqlConnector.h"
#include <jwt/jwt.hpp>
#include <exception>


std::string getHeader(const std::shared_ptr<HttpServerImpl::Request> &request, const std::string &header) {
    // Iterate over the headers
    for (const auto &h : request->header) {
        // Check if the header matches
        if (h.first == header) {
            // Return the header value
            return h.second;
        }
    }

    // Return an empty string
    return std::string();
}

nlohmann::json isAuthorized(const std::shared_ptr<HttpServerImpl::Request> &request) {
    // Get the Authorization header from the request
    auto jwt = getHeader(request, "Authorization");

    // Check if the header existed
    if (jwt.empty())
        // Not authorized
        throw std::exception();

    // Decode the token
    std::error_code ec;
    auto dec_obj = jwt::decode(jwt, jwt::params::algorithms({"HS256"}), ec, jwt::params::secret(JWT_SECRET),
                               jwt::params::verify(true));

    // If there is any error code, the user is not authorized
    if (ec)
        throw std::exception();

    // Everything is fine
    return dec_obj.payload().create_json_obj();
}