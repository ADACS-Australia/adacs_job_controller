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
#include <boost/algorithm/string/split.hpp>
#include <boost/algorithm/string/classification.hpp>

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

std::vector<uint64_t> getQueryParamAsVectorInt(SimpleWeb::CaseInsensitiveMultimap &query_fields, const std::string& what) {
    auto arrayPtr = query_fields.find(what);
    std::vector<uint64_t> intArray;
    if (arrayPtr != query_fields.end()) {
        std::vector<std::string> sIntArray;
        boost::split(sIntArray, arrayPtr->second, boost::is_any_of(", "), boost::token_compress_on);

        for (const auto &id : sIntArray) {
            intArray.push_back(stoi(id));
        }
    }
    return intArray;
}

uint64_t getQueryParamAsInt(SimpleWeb::CaseInsensitiveMultimap &query_fields, const std::string& what) {
    auto ptr = query_fields.find(what);
    uint64_t result = 0;
    if (ptr != query_fields.end())
        result = std::stoll(ptr->second);
    return result;
}

std::string getQueryParamAsString(SimpleWeb::CaseInsensitiveMultimap &query_fields, const std::string& what) {
    auto ptr = query_fields.find(what);
    std::string result;
    if (ptr != query_fields.end())
        result = ptr->second;
    return result;
}

bool hasQueryParam(SimpleWeb::CaseInsensitiveMultimap &query_fields, const std::string& what) {
    auto ptr = query_fields.find(what);
    return ptr != query_fields.end();
}