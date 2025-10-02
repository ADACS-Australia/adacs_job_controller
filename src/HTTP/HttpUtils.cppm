//
// Created by lewis on 3/11/20.
//

module;
#include <algorithm>
#include <cstdint>
#include <string>
#include <vector>

#include <jwt/jwt.hpp>
#include <server_http.hpp>

export module HttpUtils;

// Generic string splitting utility to replace boost::split
export auto splitString(const std::string& str, const std::string& delimiters) -> std::vector<std::string>
{
    std::vector<std::string> result;
    std::string::size_type start = 0;
    std::string::size_type end   = 0;

    while (end != std::string::npos)
    {
        end                     = str.find_first_of(delimiters, start);
        const std::string token = str.substr(start, (end == std::string::npos) ? std::string::npos : end - start);

        // Skip empty tokens and whitespace-only tokens
        if (!token.empty() && token.find_first_not_of(" \t") != std::string::npos)
        {
            // Trim whitespace
            auto first = token.find_first_not_of(" \t");
            auto last  = token.find_last_not_of(" \t");
            if (first != std::string::npos && last != std::string::npos)
            {
                const std::string trimmed = token.substr(first, last - first + 1);
                result.push_back(trimmed);
            }
        }

        start = (end > (std::string::npos - 1)) ? std::string::npos : end + 1;
    }

    return result;
}

export auto getHeader(SimpleWeb::CaseInsensitiveMultimap& headers, const std::string& header) -> std::string
{
    // Iterate over the headers
    for (const auto& headerItem : headers)
    {
        // Check if the header matches
        if (headerItem.first == header)
        {
            // Return the header value
            return headerItem.second;
        }
    }

    // Return an empty string
    return {};
}

// NOLINTBEGIN(clang-analyzer-cplusplus.NewDeleteLeaks)
// This lint is for a clang-tidy false positive https://bugs.llvm.org/show_bug.cgi?id=41141
export auto getQueryParamAsVectorInt(SimpleWeb::CaseInsensitiveMultimap& query_fields,
                                     const std::string& what) -> std::vector<uint64_t>
{
    auto arrayPtr = query_fields.find(what);
    std::vector<uint64_t> intArray;
    if (arrayPtr != query_fields.end())
    {
        const auto tokens = splitString(arrayPtr->second, ", ");
        for (const auto& token : tokens)
        {
            intArray.push_back(std::stoull(token));
        }
    }
    return intArray;
}

// NOLINTEND(clang-analyzer-cplusplus.NewDeleteLeaks)

export auto getQueryParamAsInt(SimpleWeb::CaseInsensitiveMultimap& query_fields, const std::string& what) -> uint64_t
{
    auto ptr        = query_fields.find(what);
    uint64_t result = 0;
    // NOLINTNEXTLINE(clang-analyzer-security.ArrayBound)
    if (ptr != query_fields.end())
    {
        result = std::stoll(ptr->second);
    }
    return result;
}

export auto getQueryParamAsString(SimpleWeb::CaseInsensitiveMultimap& query_fields,
                                  const std::string& what) -> std::string
{
    auto ptr = query_fields.find(what);
    std::string result;
    // NOLINTNEXTLINE(clang-analyzer-security.ArrayBound)
    if (ptr != query_fields.end())
    {
        result = ptr->second;
    }
    return result;
}

export auto hasQueryParam(SimpleWeb::CaseInsensitiveMultimap& query_fields, const std::string& what) -> bool
{
    auto ptr = query_fields.find(what);
    // NOLINTNEXTLINE(clang-analyzer-security.ArrayBound)
    return ptr != query_fields.end();
}
