cmake_minimum_required(VERSION 3.13)
project(gwcloud_job_server)

set(CMAKE_EXPORT_COMPILE_COMMANDS ON)
set(CMAKE_CXX_STANDARD 20)

# Generate maximum debug information (Dwarf version 4 for folly backtrace support)
set(CMAKE_CXX_FLAGS_DEBUG " -g -O0 -rdynamic -gdwarf-4 -fpermissive -w ")

# Suppress nested inexact package name searches
set(FPHSA_NAME_MISMATCHED 1)

# We want to control variables in the child projects from here
set(CMAKE_POLICY_DEFAULT_CMP0077 NEW)

# Set up boost
set(Boost_USE_STATIC_LIBS OFF)
set(Boost_USE_MULTITHREAD ON)
find_package(Boost 1.67 REQUIRED COMPONENTS unit_test_framework)
link_libraries(${Boost_UNIT_TEST_FRAMEWORK_LIBRARY})

# Make sure we find mysql
set(CMAKE_MODULE_PATH ${CMAKE_MODULE_PATH} ${CMAKE_CURRENT_SOURCE_DIR}/cmake)
find_package(MySQL REQUIRED)

# Set up the hinnant date library
set(HinnantDate_ROOT_DIR Lib/date)
set(HinnantDate_INCLUDE_DIR Lib/date/include)

# Set config for nested libraries
set(ENABLE_TESTS Off)
set(ENABLE_DATE_TESTING Off)
set(USE_OPENSSL Off)
set(CPP_JWT_BUILD_TESTS Off)
set(CPP_JWT_BUILD_EXAMPLES Off)
set(CMAKE_LIBRARY_ARCHITECTURE "x86_64-linux-gnu")
set(BUILD_MYSQL_CONNECTOR ON)

# Include the nested libraries
add_subdirectory(third_party/Simple-Web-Server)
add_subdirectory(third_party/Simple-WebSocket-Server)
add_subdirectory(third_party/folly)
add_subdirectory(third_party/sqlpp11)
add_subdirectory(third_party/cpp-jwt)

# Set up the include directories
include_directories(
        third_party/json/include
        third_party/date/include
        third_party/sqlpp11/include
        /usr/include/mysql
        third_party/folly
        third_party/Simple-Web-Server
        third_party/Simple-WebSocket-Server
        third_party/cpp-jwt/include
        ${MYSQL_INCLUDE_DIR}
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
        Settings.h
        Lib/JobStatus.h
        HTTP/HttpUtils.cpp
        HTTP/File.cpp
        Lib/segvcatch.h
        Lib/segvcatch.cpp
        Lib/x86_64-signal.h
        Lib/GeneralUtils.h
        Lib/GeneralUtils.cpp
        HTTP/Utils/HandleFileList.cpp
        DB/ClusterDB.cpp
        DB/ClusterDB.h
        DB/sClusterJob.h
        DB/sClusterJobStatus.h
        Cluster/FileDownload.cpp
        Cluster/FileDownload.h
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
        tests/utils.cpp
        tests/test_main.cpp
        Lib/Messaging/tests.cpp
        Cluster/tests/cluster_tests.cpp
        Cluster/tests/cluster_manager_tests.cpp
        HTTP/tests/httpserver_tests.cpp
        HTTP/tests/job_tests.cpp
        HTTP/tests/file_tests.cpp
        tests/test_file_transfer.cpp
        tests/test_file_list_caching.cpp
        HTTP/tests/file_filter_files.cpp
        tests/fixtures/DatabaseFixture.h
        tests/fixtures/HttpServerFixture.h
        tests/fixtures/HttpClientFixture.h
        tests/fixtures/WebSocketClientFixture.h
        tests/fixtures/WebSocketServerFixture.h
        Cluster/tests/ping_pong_tests.cpp
        Cluster/tests/cluster_db_tests.cpp
        Cluster/tests/file_download_tests.cpp
)

# Set the link libraries
set(
        LINK_LIBRARIES
        simple-web-server
        simple-websocket-server
        folly
        folly_exception_tracer_base
        folly_exception_tracer
        folly_exception_counter
        dl
        dw
        gcov
        mysqlclient
)

set_target_properties(
        Boost_Tests_run
        PROPERTIES
        CXX_CLANG_TIDY "clang-tidy"
        CXX_FLAGS_DEBUG " -g -O0 -rdynamic -gdwarf-4 -fpermissive -Wall "
)

set_target_properties(
        gwcloud_job_server
        PROPERTIES
        CXX_CLANG_TIDY "clang-tidy"
        CXX_FLAGS_DEBUG " -g -O0 -rdynamic -gdwarf-4 -fpermissive -Wall "
)

target_link_libraries(gwcloud_job_server ${LINK_LIBRARIES})
target_link_libraries(Boost_Tests_run ${LINK_LIBRARIES})

# Configure custom definitions for testing
target_compile_definitions(Boost_Tests_run PUBLIC -DBUILD_TESTS)
target_compile_options(Boost_Tests_run PUBLIC -fprofile-arcs -ftest-coverage -fPIC)