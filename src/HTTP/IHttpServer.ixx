//
// Created by lewis on 3/11/20.
//

module;

#include <string>
#include <memory>
#include <server_http.hpp>

export module IHttpServer;

import IServer;

// Forward declarations
class IApplication;

export using HttpServerImpl = SimpleWeb::Server<SimpleWeb::HTTP>;

export class IHttpServer : public IServer {
public:
    // No API registration methods needed - endpoints are registered directly in Application
};
