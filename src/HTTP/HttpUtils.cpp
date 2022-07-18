//
// Created by lewis on 3/11/20.
//

#include "HttpUtils.h"
#include <boost/algorithm/string/classification.hpp>
#include <boost/algorithm/string/split.hpp>
#include <jwt/jwt.hpp>

auto getHeader(SimpleWeb::CaseInsensitiveMultimap& headers, const std::string &header) -> std::string {
    // Iterate over the headers
    for (const auto &headerItem : headers) {
        // Check if the header matches
        if (headerItem.first == header) {
            // Return the header value
            return headerItem.second;
        }
    }

    // Return an empty string
    return {};
}

// NOLINTBEGIN(clang-analyzer-cplusplus.NewDeleteLeaks)
// This lint is for a clang-tidy false positive https://bugs.llvm.org/show_bug.cgi?id=41141
auto getQueryParamAsVectorInt(SimpleWeb::CaseInsensitiveMultimap &query_fields, const std::string& what) -> std::vector<uint64_t> {
    auto arrayPtr = query_fields.find(what);
    std::vector<uint64_t> intArray;
    if (arrayPtr != query_fields.end()) {
        std::vector<std::string> sIntArray;
        boost::split(sIntArray, arrayPtr->second, boost::is_any_of(", "), boost::token_compress_on);

        for (const auto &item : sIntArray) {
            intArray.push_back(stoi(item));
        }
    }
    return intArray;
}
// NOLINTEND(clang-analyzer-cplusplus.NewDeleteLeaks)

auto getQueryParamAsInt(SimpleWeb::CaseInsensitiveMultimap &query_fields, const std::string& what) -> uint64_t {
    auto ptr = query_fields.find(what);
    uint64_t result = 0;
    if (ptr != query_fields.end()) {
        result = std::stoll(ptr->second);
    }
    return result;
}

auto getQueryParamAsString(SimpleWeb::CaseInsensitiveMultimap &query_fields, const std::string& what) -> std::string {
    auto ptr = query_fields.find(what);
    std::string result;
    if (ptr != query_fields.end()) {
        result = ptr->second;
    }
    return result;
}

auto hasQueryParam(SimpleWeb::CaseInsensitiveMultimap &query_fields, const std::string& what) -> bool {
    auto ptr = query_fields.find(what);
    return ptr != query_fields.end();
}