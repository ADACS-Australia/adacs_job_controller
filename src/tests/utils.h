#include <iostream>
#include <vector>
#include <server_ws.hpp>
#include <client_ws.hpp>
#include <client_http.hpp>
#include <server_http.hpp>
#include <server_https.hpp>
#include "../HTTP/HttpServer.h"
#include "../Lib/jobserver_schema.h"
#include "../DB/MySqlConnector.h"

std::string getLastToken();
extern uint64_t randomInt(uint64_t start, uint64_t end);
extern std::shared_ptr<std::vector<uint8_t>> generateRandomData(uint32_t count);

using TestWsServer = SimpleWeb::SocketServer<SimpleWeb::WS>;
using TestWsClient = SimpleWeb::SocketClient<SimpleWeb::WS>;

using TestHttpServer = SimpleWeb::Server<SimpleWeb::HTTP>;
using TestHttpsServer = SimpleWeb::Server<SimpleWeb::HTTPS>;

using TestHttpClient = SimpleWeb::Client<SimpleWeb::HTTP>;
using TestHttpsClient = SimpleWeb::Client<SimpleWeb::HTTPS>;
