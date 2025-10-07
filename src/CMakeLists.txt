cmake_minimum_required(VERSION 3.28)

# Set Clang 22 compiler for C++20 module support
set(CMAKE_CXX_COMPILER clang++-22)
set(CMAKE_C_COMPILER clang-22)

project(adacs_job_controller)

# Option to enable/disable clang-tidy
option(ENABLE_CLANG_TIDY "Enable clang-tidy static analysis" ON)

if(ENABLE_CLANG_TIDY)
    message(STATUS "clang-tidy static analysis: ENABLED")
else()
    message(STATUS "clang-tidy static analysis: DISABLED")
endif()

set(CMAKE_EXPORT_COMPILE_COMMANDS ON)
set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

# Generate maximum debug information (Dwarf version 4 for folly backtrace support)
set(CMAKE_CXX_FLAGS_DEBUG " -g -O0 -gdwarf-4 -fpermissive -w ")

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

# Find system cpp-jwt package
find_package(cpp-jwt REQUIRED)

# Find date library
find_package(date REQUIRED)

# Include the nested libraries
add_subdirectory(third_party/Simple-Web-Server)
add_subdirectory(third_party/Simple-WebSocket-Server)
add_subdirectory(third_party/folly)
add_subdirectory(third_party/sqlpp11)

# Define common include directories
set(COMMON_INCLUDE_DIRS
        /usr/include/nlohmann
        /usr/include/date
        third_party/sqlpp11/include
        /usr/include/mysql
        third_party/folly
        third_party/Simple-Web-Server
        third_party/Simple-WebSocket-Server
        ${MYSQL_INCLUDE_DIR}
        ${CMAKE_CURRENT_BINARY_DIR}/third_party/folly
)

# Set up the include directories
include_directories(${COMMON_INCLUDE_DIRS})

# Define common compile features
set(COMMON_COMPILE_FEATURES cxx_std_20)

# Define the source files
set(
        SOURCE_FILES
        Lib/segvcatch.cpp
        Lib/shims/sqlpp_shim.h
        Lib/shims/date_shim.h
        Lib/TestingMacros.h
)


# Set the main executable and source files
add_executable(
        adacs_job_controller
	${SOURCE_FILES}
        main.cpp
)
target_compile_features(adacs_job_controller PUBLIC ${COMMON_COMPILE_FEATURES})

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
        tests/test_file_upload_transfer.cpp
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
        Cluster/tests/file_upload_tests.cpp
        Cluster/tests/background_threads_tests.cpp
)
target_compile_features(Boost_Tests_run PUBLIC ${COMMON_COMPILE_FEATURES})

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
        mysqlclient
        cpp-jwt::cpp-jwt
        date::date-tz
)

# Define module sources once
set(PROJECT_MODULE_SOURCES
  IApplication.ixx
  Application.cppm
  Interfaces/IServer.ixx
  HTTP/IHttpServer.ixx
  HTTP/HttpServer.cppm
  HTTP/HttpUtils.cppm
  HTTP/File.cppm
  HTTP/Job.cppm
  WebSocket/IWebSocketServer.ixx
  WebSocket/WebSocketServer.cppm
  Lib/JobStatus.ixx
  Lib/GeneralUtils.cppm
  Settings.ixx
  Lib/Messaging/Message.cppm
  Lib/jobserver_schema.ixx
  DB/MySqlConnector.ixx
  DB/sClusterJob.cppm
  DB/sClusterJobStatus.cppm
  DB/sBundleJob.cppm
  DB/ClusterDB.cppm
  Cluster/Cluster.cppm
  Cluster/ClusterManager.cppm
  Cluster/FileDownload.cppm
  Cluster/FileUpload.cppm
  Interfaces/ICluster.ixx
  Interfaces/IClusterManager.ixx
  HTTP/Utils/HandleFileList.cppm
)

# Production modules (no BUILD_TESTS)
add_library(project_modules)
target_compile_features(project_modules PUBLIC ${COMMON_COMPILE_FEATURES})
target_sources(project_modules
  PUBLIC
    FILE_SET cxx_modules TYPE CXX_MODULES FILES
      ${PROJECT_MODULE_SOURCES}
)

# Test modules (with BUILD_TESTS)
add_library(project_modules_test)
target_compile_features(project_modules_test PUBLIC ${COMMON_COMPILE_FEATURES})
target_sources(project_modules_test
  PUBLIC
    FILE_SET cxx_modules TYPE CXX_MODULES FILES
      ${PROJECT_MODULE_SOURCES}
)
target_compile_definitions(project_modules_test PUBLIC -DBUILD_TESTS)
target_compile_options(project_modules_test PUBLIC --coverage -fPIC)

# Link other libraries
target_link_libraries(adacs_job_controller PRIVATE ${LINK_LIBRARIES} project_modules)
target_link_libraries(Boost_Tests_run PRIVATE ${LINK_LIBRARIES} project_modules_test)
target_compile_options(Boost_Tests_run PUBLIC --coverage -fPIC)

target_link_options(Boost_Tests_run PRIVATE --coverage)

set_target_properties(
        Boost_Tests_run
        PROPERTIES
        CXX_FLAGS_DEBUG " -g -O0 -rdynamic -gdwarf-4 -fpermissive -Wall "
)

if(ENABLE_CLANG_TIDY)
    set_target_properties(
            adacs_job_controller 
            PROPERTIES
            CXX_CLANG_TIDY "clang-tidy-22;--extra-arg-before=--driver-mode=g++;--enable-module-headers-parsing"
            CXX_FLAGS_DEBUG " -g -O0 -rdynamic -gdwarf-4 -fpermissive -Wall "
    )
else()
    set_target_properties(
            adacs_job_controller 
            PROPERTIES
            CXX_FLAGS_DEBUG " -g -O0 -rdynamic -gdwarf-4 -fpermissive -Wall "
    )
endif()

if(ENABLE_CLANG_TIDY)
    # Enable clang-tidy-22 with experimental module support for project_modules
    set_target_properties(
            project_modules 
            PROPERTIES
            CXX_CLANG_TIDY "clang-tidy-22;--extra-arg-before=--driver-mode=g++;--enable-module-headers-parsing"
            CXX_FLAGS_DEBUG " -g -O0 -rdynamic -gdwarf-4 -fpermissive -Wall "
    )
else()
    set_target_properties(
            project_modules 
            PROPERTIES
            CXX_FLAGS_DEBUG " -g -O0 -rdynamic -gdwarf-4 -fpermissive -Wall "
    )
endif()

if(ENABLE_CLANG_TIDY)
    # Enable clang-tidy-22 with experimental module support for project_modules_test
    set_target_properties(
            project_modules_test 
            PROPERTIES
            CXX_CLANG_TIDY "clang-tidy-22;--extra-arg-before=--driver-mode=g++;--enable-module-headers-parsing"
            CXX_FLAGS_DEBUG " -g -O0 -rdynamic -gdwarf-4 -fpermissive -Wall "
    )
else()
    set_target_properties(
            project_modules_test 
            PROPERTIES
            CXX_FLAGS_DEBUG " -g -O0 -rdynamic -gdwarf-4 -fpermissive -Wall "
    )
endif()