cmake_minimum_required(VERSION 3.16)
project(gwcloud_job_server)

set(CMAKE_CXX_STANDARD 20)

SET(Boost_USE_STATIC_LIBS OFF)
SET(Boost_USE_MULTITHREAD ON)
FIND_PACKAGE(Boost 1.72 REQUIRED COMPONENTS unit_test_framework)
IF(Boost_FOUND)
    INCLUDE_DIRECTORIES(${Boost_INCLUDE_DIRS})
    LINK_DIRECTORIES(${Boost_LIBRARY_DIRS})
ENDIF(Boost_FOUND)
SET(USED_LIBS ${Boost_UNIT_TEST_FRAMEWORK_LIBRARY})
link_libraries(${Boost_UNIT_TEST_FRAMEWORK_LIBRARY})

set(USE_OPENSSL OFF)

add_subdirectory(Lib/Simple-Web-Server/)
add_subdirectory(Lib/Simple-WebSocket-Server/)
add_subdirectory(Lib/folly/)
include_directories(Lib/folly/)

add_executable(gwcloud_job_server main.cpp HTTP/HttpServer.cpp HTTP/HttpServer.h WebSocket/WebSocketServer.cpp WebSocket/WebSocketServer.h Lib/Messaging/Message.cpp Lib/Messaging/Message.h Cluster/ClusterManager.cpp Cluster/ClusterManager.h Cluster/Cluster.cpp Cluster/Cluster.h HTTP/Job.cpp)
add_executable (Boost_Tests_run Lib/Messaging/tests.cpp Lib/Messaging/Message.cpp Cluster/ClusterManager.cpp Cluster/ClusterManager.h Cluster/Cluster.cpp Cluster/Cluster.h HTTP/Job.cpp)

target_link_libraries(gwcloud_job_server simple-web-server simple-websocket-server folly)