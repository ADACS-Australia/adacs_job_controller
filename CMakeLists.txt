cmake_minimum_required(VERSION 3.15)
project(gwcloud_job_server)

set(CMAKE_CXX_STANDARD 20)

set(CMAKE_CXX_FLAGS_DEBUG " -g -O0 -rdynamic ")

SET(Boost_USE_STATIC_LIBS OFF)
SET(Boost_USE_MULTITHREAD ON)
FIND_PACKAGE(Boost 1.67 REQUIRED COMPONENTS unit_test_framework)
IF(Boost_FOUND)
    INCLUDE_DIRECTORIES(${Boost_INCLUDE_DIRS})
    LINK_DIRECTORIES(${Boost_LIBRARY_DIRS})
ENDIF(Boost_FOUND)
SET(USED_LIBS ${Boost_UNIT_TEST_FRAMEWORK_LIBRARY})
link_libraries(${Boost_UNIT_TEST_FRAMEWORK_LIBRARY})

set(HinnantDate_ROOT_DIR Lib/date)
set(HinnantDate_INCLUDE_DIR Lib/date/include)

set(ENABLE_TESTS OFF)
set(ENABLE_DATE_TESTING OFF)
set(USE_OPENSSL OFF)

add_subdirectory(Lib/Simple-Web-Server)
add_subdirectory(Lib/Simple-WebSocket-Server)
add_subdirectory(Lib/folly)
#add_subdirectory(Lib/date)
add_subdirectory(Lib/sqlpp11)
add_subdirectory(Lib/sqlpp11-connector-mysql)

include_directories(Lib/date/include Lib/sqlpp11/include Lib/sqlpp11-connector-mysql/include Lib/folly Lib/Simple-Web-Server Lib/Simple-WebSocket-Server)

add_executable(gwcloud_job_server main.cpp HTTP/HttpServer.cpp HTTP/HttpServer.h WebSocket/WebSocketServer.cpp WebSocket/WebSocketServer.h Lib/Messaging/Message.cpp Lib/Messaging/Message.h Cluster/ClusterManager.cpp Cluster/ClusterManager.h Cluster/Cluster.cpp Cluster/Cluster.h HTTP/Job.cpp DB/MySqlConnector.h Lib/stacktrace.cpp)
add_executable (Boost_Tests_run Lib/Messaging/tests.cpp Lib/Messaging/Message.cpp Cluster/ClusterManager.cpp Cluster/ClusterManager.h Cluster/Cluster.cpp Cluster/Cluster.h HTTP/Job.cpp Lib/stacktrace.cpp)

target_link_libraries(gwcloud_job_server simple-web-server simple-websocket-server folly sqlpp-mysql mysqlclient dl dw)