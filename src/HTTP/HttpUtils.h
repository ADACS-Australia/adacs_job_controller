//
// Created by lewis on 3/11/20.
//

#ifndef GWCLOUD_JOB_SERVER_HTTPUTILS_H
#define GWCLOUD_JOB_SERVER_HTTPUTILS_H

#include "HttpServer.h"

auto getHeader(SimpleWeb::CaseInsensitiveMultimap& headers, const std::string &header) -> std::string;
auto getQueryParamAsVectorInt(SimpleWeb::CaseInsensitiveMultimap& query_fields, const std::string& what) -> std::vector<uint64_t>;
auto getQueryParamAsInt(SimpleWeb::CaseInsensitiveMultimap& query_fields, const std::string& what) -> uint64_t;
auto getQueryParamAsString(SimpleWeb::CaseInsensitiveMultimap& query_fields, const std::string& what) -> std::string;
auto hasQueryParam(SimpleWeb::CaseInsensitiveMultimap& query_fields, const std::string& what) -> bool;

#endif //GWCLOUD_JOB_SERVER_HTTPUTILS_H
