//
// Created by lewis on 3/11/20.
//

#ifndef GWCLOUD_JOB_SERVER_HTTPUTILS_H
#define GWCLOUD_JOB_SERVER_HTTPUTILS_H

#include "HttpServer.h"
#include <nlohmann/json.hpp>

std::string getHeader(const std::shared_ptr<HttpServerImpl::Request> &request, const std::string &header);
nlohmann::json isAuthorized(const std::shared_ptr<HttpServerImpl::Request> &request);
std::vector<uint64_t> getQueryParamAsVectorInt(SimpleWeb::CaseInsensitiveMultimap &query_fields, const std::string& what);
uint64_t getQueryParamAsInt(SimpleWeb::CaseInsensitiveMultimap &query_fields, const std::string& what);
std::string getQueryParamAsString(SimpleWeb::CaseInsensitiveMultimap &query_fields, const std::string& what);
bool hasQueryParam(SimpleWeb::CaseInsensitiveMultimap &query_fields, const std::string& what);

#endif //GWCLOUD_JOB_SERVER_HTTPUTILS_H
