//
// Created by lewis on 3/11/20.
//

#ifndef GWCLOUD_JOB_SERVER_HTTPUTILS_H
#define GWCLOUD_JOB_SERVER_HTTPUTILS_H

#include "HttpServer.h"

std::string getHeader(SimpleWeb::CaseInsensitiveMultimap& headers, const std::string &header);
std::vector<uint64_t> getQueryParamAsVectorInt(SimpleWeb::CaseInsensitiveMultimap& query_fields, const std::string& what);
uint64_t getQueryParamAsInt(SimpleWeb::CaseInsensitiveMultimap& query_fields, const std::string& what);
std::string getQueryParamAsString(SimpleWeb::CaseInsensitiveMultimap& query_fields, const std::string& what);
bool hasQueryParam(SimpleWeb::CaseInsensitiveMultimap& query_fields, const std::string& what);

#endif //GWCLOUD_JOB_SERVER_HTTPUTILS_H
