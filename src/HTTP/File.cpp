//
// Created by lewis on 3/12/20.
//
#include <jwt/jwt.hpp>
#include <boost/lexical_cast.hpp>
#include <boost/uuid/uuid_io.hpp>
#include <boost/uuid/random_generator.hpp>
#include "HttpServer.h"
#include "../DB/MySqlConnector.h"
#include "../Lib/jobserver_schema.h"
#include "../Cluster/ClusterManager.h"
#include "HttpUtils.h"

using namespace std;
using namespace schema;

void FileApi(const std::string &path, HttpServerImpl *server, ClusterManager *clusterManager) {
    // Get      -> Download file (file uuid)
    // Post     -> Create new file download
    // Delete   -> Delete file download (file uuid)
    // Patch    -> List files in directory

    // Create a new file download
    server->resource["^" + path + "$"]["POST"] = [clusterManager](shared_ptr<HttpServerImpl::Response> response,
                                                                  shared_ptr<HttpServerImpl::Request> request) {

        // Verify that the user is authorized
        nlohmann::json jwt;
        try {
            jwt = isAuthorized(request);
        } catch (...) {
            // Invalid request
            response->write(SimpleWeb::StatusCode::client_error_forbidden, "Not authorized");
            return;
        }

        // Create a database connection
        auto db = MySqlConnector();

        // Start a transaction
        db->start_transaction();

        // Get the tables
        JobserverJob jobTable;
        JobserverFiledownload fileDownloadTable;

        try {
            // Read the json from the post body
            nlohmann::json post_data;
            request->content >> post_data;

            // Get the job to fetch files for
            auto jobId = post_data["jobId"];

            // Get the path to the file to fetch (relative to the project)
            auto filePath = post_data["path"];

            // Look up the job
            auto jobResults =
                    db->run(
                            select(all_of(jobTable))
                                    .from(jobTable)
                                    .where(jobTable.id == (uint32_t) jobId)
                    );

            // Get the cluster and bundle from the job
            std::string sCluster, sBundle;

            // There's only one job, but I found I had to iterate it to get the single record, as front gave me corrupted results
            for (auto &job : jobResults) {
                sCluster = std::string(job.cluster);
                sBundle = std::string(job.bundle);
            }

            // Generate a UUID for the download
            auto uuid = boost::lexical_cast<std::string>(boost::uuids::random_generator()());

            // Save the file download in the database
            db->run(
                    insert_into(fileDownloadTable)
                            .set(
                                    fileDownloadTable.user = (uint32_t) jwt["userId"],
                                    fileDownloadTable.jobId = (uint32_t) jobId,
                                    fileDownloadTable.uuid = uuid,
                                    fileDownloadTable.path = std::string(filePath),
                                    fileDownloadTable.timestamp = std::chrono::system_clock::now()
                            )
            );

            // Commit the changes in the database
            db->commit_transaction();

            // Report success
            nlohmann::json result;
            result["fileId"] = uuid;

            SimpleWeb::CaseInsensitiveMultimap headers;
            headers.emplace("Content-Type", "application/json");

            response->write(SimpleWeb::StatusCode::success_ok, result.dump(), headers);

        } catch (...) {
            // Abort the transaction
            db->rollback_transaction(false);

            // Report bad request
            response->write(SimpleWeb::StatusCode::client_error_bad_request, "Bad request");
        }
    };

    // Download file
    server->resource["^" + path + "$"]["GET"] = [clusterManager](shared_ptr<HttpServerImpl::Response> response,
                                                                 shared_ptr<HttpServerImpl::Request> request) {

        // Create a database connection
        auto db = MySqlConnector();

        // Get the tables
        JobserverJob jobTable;
        JobserverFiledownload fileDownloadTable;

        // Create a new file download object
        auto fdObj = new sFileDownload{};
        std::string uuid;

        try {
            // Process the query parameters
            auto query_fields = request->parse_query_string();

            // Check if jobId is provided
            auto uuidPtr = query_fields.find("fileId");
            if (uuidPtr != query_fields.end())
                uuid = uuidPtr->second;

            if (uuid.empty()) {
                response->write(SimpleWeb::StatusCode::client_error_bad_request, "Bad Request");
                return;
            }

            // Check if forceDownload is provided
            auto fdPtr = query_fields.find("forceDownload");
            bool forceDownload = false;
            if (fdPtr != query_fields.end())
                forceDownload = true;

            // todo: expire old downloads

            // Look up the file download
            auto dlResults = db->run(
                    select(all_of(fileDownloadTable))
                            .from(fileDownloadTable)
                            .where(fileDownloadTable.uuid == uuid)
            );

            // Check that the uuid existed in the database
            if (dlResults.empty()) {
                response->write(SimpleWeb::StatusCode::client_error_bad_request, "Bad Request");
                return;
            }

            // Get the cluster and bundle from the job
            std::string sCluster, sBundle, sFilePath;
            uint32_t jobId = 0;

            // I found I had to iterate it to get the single record, as front gave me corrupted results
            for (auto &dl : dlResults) {
                // Look up the job
                auto jobResults =
                        db->run(
                                select(all_of(jobTable))
                                        .from(jobTable)
                                        .where(jobTable.id == (uint32_t) dl.jobId)
                        );

                for (auto &job : jobResults) {
                    sCluster = std::string(job.cluster);
                    sBundle = std::string(job.bundle);
                }

                sFilePath = std::string(dl.path);
                jobId = (uint32_t) dl.jobId;
            }

            // Check that the cluster is online
            // Get the cluster to submit to
            auto cluster = clusterManager->getCluster(sCluster);

            // If the cluster isn't online then there isn't anything to do
            if (!cluster->isOnline()) {
                response->write(SimpleWeb::StatusCode::server_error_service_unavailable, "Remote Cluster Offline");
                return;
            }

            // Cluster is online, create the file hash object
            fileDownloadMap.emplace(uuid, fdObj);

            // Send a message to the client to initiate the file download
            auto msg = Message(DOWNLOAD_FILE, Message::Priority::Highest, uuid);
            msg.push_uint(jobId);
            msg.push_string(uuid);
            msg.push_string(sBundle);
            msg.push_string(sFilePath);
            msg.send(cluster);

            {
                // Wait for the server to send back data, or in 10 seconds fail
                std::unique_lock<std::mutex> lock(fdObj->dataCVMutex);

                // Wait for data to be ready to send
                fdObj->dataCV.wait_for(lock, std::chrono::seconds(10), [fdObj] { return fdObj->dataReady; });
            }
            // Check if the server received an error about the file
            if (fdObj->error) {
                response->write(SimpleWeb::StatusCode::client_error_bad_request, fdObj->errorDetails);
                return;
            }

            // Check if the server received details about the file
            if (!fdObj->receivedData) {
                response->write(SimpleWeb::StatusCode::server_error_service_unavailable, "Remote Cluster Offline");
                return;
            }

            // Send the file size back to the client
            nlohmann::json result;
            result["fileId"] = uuid;

            SimpleWeb::CaseInsensitiveMultimap headers;
            headers.emplace("Content-Type", "application/octet-stream");
            // Check if we need to tell the browser to force the download
            if (forceDownload)
                headers.emplace("Content-Disposition", "application/octet-stream");
            // Set the content size
            headers.emplace("Content-Length", std::to_string(fdObj->fileSize));

            // Write the headers
            response->write(headers);

            // Check for error, or all data sent
            while (!fdObj->error && fdObj->sentBytes < fdObj->fileSize) {
                {
                    // Wait for the server to send back data, or in 10 seconds fail
                    std::unique_lock<std::mutex> lock(fdObj->dataCVMutex);

                    // Wait for data to be ready to send
                    fdObj->dataCV.wait_for(lock, std::chrono::seconds(10), [fdObj] { return fdObj->dataReady; });
                    fdObj->dataReady = false;
                }

                // While there is data in the queue, send to the client
                while (!fdObj->queue.empty()) {
                    // Get the next chunk from the queue
                    auto data = fdObj->queue.try_dequeue();

                    // If there was one, send it to the client
                    if (data) {
                        // Increment the traffic counter
                        fdObj->sentBytes += (*data)->size();

                        // Send the data
                        response->write((const char*) (*data)->data(), (*data)->size());

                        // Clean up the data
                        delete (*data);

                        // Check if we need to resume the client file transfer
                        if (fdObj->clientPaused) {
                            // Check if the buffer is smaller than the setting
                            if (fdObj->receivedBytes - fdObj->sentBytes < MIN_FILE_BUFFER_SIZE) {
                                // Ask the client to resume the file transfer
                                fdObj->clientPaused = false;

                                auto resumeMsg = Message(RESUME_FILE_CHUNK_STREAM, Message::Priority::Highest, uuid);
                                resumeMsg.push_string(uuid);
                                resumeMsg.send(cluster);
                            }
                        }
                    }
                }
            }
        } catch (...) {
            // Destroy the file download object
            if (fileDownloadMap.find(uuid) != fileDownloadMap.end()) {
                fileDownloadMap.erase(uuid);
                delete fdObj;
            }

            // Report bad request
            response->write(SimpleWeb::StatusCode::client_error_bad_request, "Bad request");
        }
    };

    // List files in the specified directory
    server->resource["^" + path + "$"]["PATCH"] = [clusterManager](shared_ptr<HttpServerImpl::Response> response,
                                                                  shared_ptr<HttpServerImpl::Request> request) {

        // Verify that the user is authorized
        nlohmann::json jwt;
        try {
            jwt = isAuthorized(request);
        } catch (...) {
            // Invalid request
            response->write(SimpleWeb::StatusCode::client_error_forbidden, "Not authorized");
            return;
        }

        // Create a database connection
        auto db = MySqlConnector();

        // Get the tables
        JobserverJob jobTable;

        // Create a new file download object
        auto flObj = new sFileList{};

        try {
            // Read the json from the post body
            nlohmann::json post_data;
            request->content >> post_data;

            // Get the job to fetch files for
            auto jobId = post_data["jobId"];

            // Get the job to fetch files for
            auto bRecursive = post_data["recursive"];

            // Get the path to the file to fetch (relative to the project)
            auto filePath = post_data["path"];

            // Look up the job
            auto jobResults =
                    db->run(
                            select(all_of(jobTable))
                                    .from(jobTable)
                                    .where(jobTable.id == (uint32_t) jobId)
                    );

            // Get the cluster and bundle from the job
            std::string sCluster, sBundle;

            // There's only one job, but I found I had to iterate it to get the single record, as front gave me corrupted results
            for (auto &job : jobResults) {
                sCluster = std::string(job.cluster);
                sBundle = std::string(job.bundle);
            }

            // Check that the cluster is online
            // Get the cluster to submit to
            auto cluster = clusterManager->getCluster(sCluster);

            // If the cluster isn't online then there isn't anything to do
            if (!cluster->isOnline()) {
                response->write(SimpleWeb::StatusCode::server_error_service_unavailable, "Remote Cluster Offline");
                return;
            }

            // Generate a UUID for the message source
            auto uuid = boost::lexical_cast<std::string>(boost::uuids::random_generator()());

            // Cluster is online, create the file list object
            fileListMap.emplace(uuid, flObj);

            // Send a message to the client to initiate the file list
            auto msg = Message(FILE_LIST, Message::Priority::Highest, uuid);
            msg.push_uint(jobId);
            msg.push_string(uuid);
            msg.push_string(sBundle);
            msg.push_string(filePath);
            msg.push_bool(bRecursive);
            msg.send(cluster);

            {
                // Wait for the server to send back data, or in 10 seconds fail
                std::unique_lock<std::mutex> lock(flObj->dataCVMutex);

                // Wait for data to be ready to send
                flObj->dataCV.wait_for(lock, std::chrono::seconds(10), [flObj] { return flObj->dataReady; });
            }

            // Check if the server received an error about the file list
            if (flObj->error) {
                response->write(SimpleWeb::StatusCode::client_error_bad_request, flObj->errorDetails);
                return;
            }

            // Iterate over the files and generate the result
            nlohmann::json result;
            result["files"] = nlohmann::json::array();
            for (auto f : flObj->files) {
                nlohmann::json jFile;
                jFile["path"] = f.fileName;
                jFile["isDir"] = f.isDirectory;
                jFile["fileSize"] = f.fileSize;
                jFile["permissions"] = f.permissions;

                result["files"].push_back(jFile);
            }

            // Return the result
            SimpleWeb::CaseInsensitiveMultimap headers;
            headers.emplace("Content-Type", "application/json");

            response->write(SimpleWeb::StatusCode::success_ok, result.dump(), headers);
        } catch (...) {
            // Report bad request
            response->write(SimpleWeb::StatusCode::client_error_bad_request, "Bad request");
        }
    };
}