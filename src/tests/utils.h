#ifndef GWCLOUD_JOB_SERVER_UTILS_H
#define GWCLOUD_JOB_SERVER_UTILS_H

#include <iostream>
#include <limits>
#include <vector>

#include <client_http.hpp>
#include <client_ws.hpp>
#include <server_http.hpp>
#include <server_https.hpp>
#include <server_ws.hpp>

auto getLastToken() -> std::string;
auto randomInt(uint64_t start, uint64_t end) -> uint64_t;
auto generateRandomData(uint32_t count) -> std::shared_ptr<std::vector<uint8_t>>;

using TestWsServer = SimpleWeb::SocketServer<SimpleWeb::WS>;
using TestWsClient = SimpleWeb::SocketClient<SimpleWeb::WS>;

using TestHttpServer  = SimpleWeb::Server<SimpleWeb::HTTP>;
using TestHttpsServer = SimpleWeb::Server<SimpleWeb::HTTPS>;

using TestHttpClient  = SimpleWeb::Client<SimpleWeb::HTTP>;
using TestHttpsClient = SimpleWeb::Client<SimpleWeb::HTTPS>;

#endif  // GWCLOUD_JOB_SERVER_UTILS_H