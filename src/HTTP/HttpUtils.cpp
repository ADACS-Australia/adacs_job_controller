//
// Created by lewis on 3/11/20.
//

#include "HttpUtils.h"
#include <jwt/jwt.hpp>
#include <boost/algorithm/string/split.hpp>
#include <boost/algorithm/string/classification.hpp>

std::string getHeader(SimpleWeb::CaseInsensitiveMultimap& headers, const std::string &header) {
    // Iterate over the headers
    for (const auto &h : headers) {
        // Check if the header matches
        if (h.first == header) {
            // Return the header value
            return h.second;
        }
    }

    // Return an empty string
    return std::string();
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