cmake_minimum_required(VERSION 3.13)
project(gwcloud_job_server)

set(CMAKE_CXX_STANDARD 17)

set(CMAKE_CXX_FLAGS_DEBUG " -g -O0 -rdynamic ")

# Suppress nested inexact package name searches
set(FPHSA_NAME_MISMATCHED 1)

# We want to control variables in the child projects from here
set(CMAKE_POLICY_DEFAULT_CMP0077 NEW)

# Set up boost
SET(Boost_USE_STATIC_LIBS OFF)
SET(Boost_USE_MULTITHREAD ON)
FIND_PACKAGE(Boost 1.67 REQUIRED COMPONENTS unit_test_framework)
IF (Boost_FOUND)
    INCLUDE_DIRECTORIES(${Boost_INCLUDE_DIRS})
    LINK_DIRECTORIES(${Boost_LIBRARY_DIRS})
ENDIF (Boost_FOUND)
SET(USED_LIBS ${Boost_UNIT_TEST_FRAMEWORK_LIBRARY})
link_libraries(${Boost_UNIT_TEST_FRAMEWORK_LIBRARY})

# Set up the hinnant date library
set(HinnantDate_ROOT_DIR Lib/date)
set(HinnantDate_INCLUDE_DIR Lib/date/include)

# Set config for nested libraries
set(ENABLE_TESTS Off)
set(ENABLE_DATE_TESTING Off)
set(USE_OPENSSL Off)

# Include the nested libraries
add_subdirectory(Lib/Simple-Web-Server)
add_subdirectory(Lib/Simple-WebSocket-Server)
add_subdirectory(Lib/folly)
add_subdirectory(Lib/sqlpp11)
add_subdirectory(Lib/sqlpp11-connector-mysql)
add_subdirectory(Lib/cpp-jwt)

# Set up the include directories
include_directories(
        Lib/json/include
        Lib/date/include
        Lib/sqlpp11/include
        Lib/sqlpp11-connector-mysql/include
        Lib/folly
        Lib/Simple-Web-Server
        Lib/Simple-WebSocket-Server
        Lib/cpp-jwt/include
)

# Define the source files
set(
        SOURCE_FILES
        HTTP/HttpServer.cpp
        HTTP/HttpServer.h
        WebSocket/WebSocketServer.cpp
        WebSocket/WebSocketServer.h
        Lib/Messaging/Message.cpp
        Lib/Messaging/Message.h
        Cluster/ClusterManager.cpp
        Cluster/ClusterManager.h
        Cluster/Cluster.cpp
        Cluster/Cluster.h
        HTTP/Job.cpp
        DB/MySqlConnector.h
        Lib/stacktrace.cpp
        Settings.h
        Lib/JobStatus.h
        HTTP/HttpUtils.cpp
        HTTP/File.cpp
        Lib/segvcatch.h
        Lib/segvcatch.cpp
        Lib/x86_64-signal.h
        Lib/GeneralUtils.h
        Lib/GeneralUtils.cpp
)

# Set the main executable and source files
add_executable(
        gwcloud_job_server
        ${SOURCE_FILES}
        main.cpp
)

# Set the test executable and source files
add_executable(
        Boost_Tests_run
        ${SOURCE_FILES}
        tests/test_main.cpp
        Lib/Messaging/tests.cpp
        Cluster/tests/cluster_tests.cpp
        Cluster/tests/cluster_manager_tests.cpp
        HTTP/tests/httpserver_tests.cpp
        HTTP/tests/job_tests.cpp
        HTTP/tests/file_tests.cpp
        tests/test_file_transfer.cpp
)

# Set the link libraries
set(
        LINK_LIBRARIES
        simple-web-server
        simple-websocket-server
        folly
        sqlpp-mysql
        mysqlclient
        dl
        dw
        gcov
)

target_link_libraries(gwcloud_job_server ${LINK_LIBRARIES})
target_link_libraries(Boost_Tests_run ${LINK_LIBRARIES})

# Configure custom definitions for testing
target_compile_definitions(Boost_Tests_run PUBLIC -DBUILD_TESTS)
target_compile_options(Boost_Tests_run PUBLIC -fprofile-arcs -ftest-coverage -fPIC)