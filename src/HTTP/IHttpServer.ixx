//
// Created by lewis on 3/11/20.
//

module;

#include <memory>
#include <string>

#include <server_http.hpp>

export module IHttpServer;

import IServer;

export using HttpServerImpl = SimpleWeb::Server<SimpleWeb::HTTP>;

export class IHttpServer : public IServer
{
public:
    // No API registration methods needed - endpoints are registered directly in Application
};
