//
// Created by lewis on 3/11/20.
//

#ifndef GWCLOUD_JOB_SERVER_HTTPUTILS_H
#define GWCLOUD_JOB_SERVER_HTTPUTILS_H

#include "HttpServer.h"
#include <nlohmann/json.hpp>

std::string getHeader(const std::shared_ptr<HttpServerImpl::Request> &request, const std::string &header);
nlohmann::json isAuthorized(const std::shared_ptr<HttpServerImpl::Request> &request);

#endif //GWCLOUD_JOB_SERVER_HTTPUTILS_H
