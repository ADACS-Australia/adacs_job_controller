//
// Created by lewis on 3/12/20.
//
#include <jwt/jwt.hpp>
#include <boost/lexical_cast.hpp>
#include <boost/uuid/uuid_io.hpp>
#include <boost/uuid/random_generator.hpp>
#include "boost/filesystem.hpp"
#include "../DB/MySqlConnector.h"
#include "../Lib/jobserver_schema.h"
#include "../Cluster/ClusterManager.h"
#include "HttpServer.h"
#include "Utils/HandleFileList.h"

using namespace schema;

void FileApi(const std::string &path, HttpServer *server, ClusterManager *clusterManager) {
    // Get      -> Download file (file uuid)
    // Post     -> Create new file download
    // Delete   -> Delete file download (file uuid)
    // Patch    -> List files in directory

    // Create a new file download
    server->getServer().resource["^" + path + "$"]["POST"] = [server](
            const std::shared_ptr<HttpServerImpl::Response> &response,
            const std::shared_ptr<HttpServerImpl::Request> &request) {

        // Verify that the user is authorized
        std::unique_ptr<sAuthorizationResult> authResult;
        try {
            authResult = server->isAuthorized(request->header);
        } catch (std::exception& e) {
            dumpExceptions(e);

            // Invalid request
            response->write(SimpleWeb::StatusCode::client_error_forbidden, "Not authorized");
            return;
        }

        // Create a database connection
        auto db = MySqlConnector();

        // Start a transaction
        db->start_transaction();

        // Create a vector which includes the application from the secret, and any other applications it has access to
        auto applications = std::vector<std::string>({authResult->secret().name()});
        std::copy(authResult->secret().applications().begin(), authResult->secret().applications().end(),
                  std::back_inserter(applications));

        // Get the tables
        JobserverJob jobTable;
        JobserverFiledownload fileDownloadTable;

        try {
            // Read the json from the post body
            nlohmann::json post_data;
            request->content >> post_data;

            // Get the job to fetch files for
            auto jobId = (uint32_t) post_data["jobId"];

            // Get the path to the file to fetch (relative to the project)
            bool hasPaths = false;
            std::vector<std::string> filePaths;
            if (post_data.contains("paths")) {
                filePaths = post_data["paths"].get<std::vector<std::string>>();
                hasPaths = true;
            } else {
                filePaths.push_back(std::string(post_data["path"]));
            }

            // Check that there were actually file paths provided
            if (filePaths.empty()) {
                // Return an empty result, this is only possible when using "paths"
                nlohmann::json result;
                result["fileIds"] = std::vector<std::string>();

                SimpleWeb::CaseInsensitiveMultimap headers;
                headers.emplace("Content-Type", "application/json");

                response->write(SimpleWeb::StatusCode::success_ok, result.dump(), headers);
                return;
            }

            // Look up the job
            auto jobResults =
                    db->run(
                            select(all_of(jobTable))
                                    .from(jobTable)
                                    .where(
                                            jobTable.id == (uint32_t) jobId
                                            and jobTable.application.in(sqlpp::value_list(applications))
                                    )
                    );

            // Check that a job was actually found
            if (jobResults.empty()) {
                throw std::runtime_error("Unable to find job with ID " + std::to_string(jobId) + " for application " +
                                         authResult->secret().name());
            }

            // Get the cluster and bundle from the job
            auto job = &jobResults.front();
            auto sCluster = std::string(job->cluster);
            auto sBundle = std::string(job->bundle);

            // Create a multi insert query
            auto insert_query = insert_into(fileDownloadTable).columns(
                    fileDownloadTable.user,
                    fileDownloadTable.jobId,
                    fileDownloadTable.uuid,
                    fileDownloadTable.path,
                    fileDownloadTable.timestamp
            );

            // Now iterate over the file paths and generate UUID's for them
            std::vector<std::string> uuids;
            for (const auto &path : filePaths) {
                // Generate a UUID for the download
                auto uuid = boost::lexical_cast<std::string>(boost::uuids::random_generator()());

                // Save the path
                uuids.push_back(uuid);

                // Add the record to be inserted
                insert_query.values.add(
                        fileDownloadTable.user = (int) authResult->payload()["userId"],
                        fileDownloadTable.jobId = (int) jobId,
                        fileDownloadTable.uuid = uuid,
                        fileDownloadTable.path = std::string(path),
                        fileDownloadTable.timestamp =
                                std::chrono::time_point_cast<std::chrono::microseconds>(
                                        std::chrono::system_clock::now()
                                )
                );
            }

            // Try inserting the values in the database
            try {
                db->run(insert_query);

                // Commit the changes in the database
                db->commit_transaction();
            } catch (sqlpp::exception &e) {
                dumpExceptions(e);

                // Uh oh, an error occurred
                // Abort the transaction
                db->rollback_transaction(false);

                // Report bad request
                response->write(
                        SimpleWeb::StatusCode::client_error_bad_request,
                        "Unable to insert records in the database, please try again later"
                );
                return;
            }

            // Report success
            nlohmann::json result;
            if (hasPaths) {
                result["fileIds"] = uuids;
            } else {
                result["fileId"] = uuids[0];
            }

            SimpleWeb::CaseInsensitiveMultimap headers;
            headers.emplace("Content-Type", "application/json");

            response->write(SimpleWeb::StatusCode::success_ok, result.dump(), headers);

        } catch (std::exception& e) {
            dumpExceptions(e);

            // Abort the transaction
            db->rollback_transaction(false);

            // Report bad request
            response->write(SimpleWeb::StatusCode::client_error_bad_request, "Bad request");
        }
    };

    // Download file
    server->getServer().resource["^" + path + "$"]["GET"] = [clusterManager](
            const std::shared_ptr<HttpServerImpl::Response> &response,
            const std::shared_ptr<HttpServerImpl::Request> &request) {

        // Create a database connection
        auto db = MySqlConnector();

        // Get the tables
        JobserverJob jobTable;
        JobserverFiledownload fileDownloadTable;

        // Create a new file download object
        auto fdObj = sFileDownload{};
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

            // Expire any old file downloads
            db->run(
                    remove_from(fileDownloadTable)
                            .where(
                                    fileDownloadTable.timestamp <=
                                    std::chrono::system_clock::now() -
                                    std::chrono::seconds(FILE_DOWNLOAD_EXPIRY_TIME)
                            )

            );

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
            auto dl = &dlResults.front();

            // Look up the job
            auto jobResults =
                    db->run(
                            select(all_of(jobTable))
                                    .from(jobTable)
                                    .where(jobTable.id == (uint32_t) dl->jobId)
                    );

            auto job = &jobResults.front();
            auto sCluster = std::string(job->cluster);
            auto sBundle = std::string(job->bundle);

            auto sFilePath = std::string(dl->path);
            auto jobId = (uint32_t) dl->jobId;

            // Check that the cluster is online
            // Get the cluster to submit to
            auto cluster = clusterManager->getCluster(sCluster);
            if (!cluster) {
                // Invalid cluster
                throw std::runtime_error("Invalid cluster");
            }

            // If the cluster isn't online then there isn't anything to do
            if (!cluster->isOnline()) {
                response->write(SimpleWeb::StatusCode::server_error_service_unavailable, "Remote Cluster Offline");
                return;
            }

            // Cluster is online, create the file hash object
            fileDownloadMap.emplace(uuid, &fdObj);

            // Send a message to the client to initiate the file download
            auto msg = Message(DOWNLOAD_FILE, Message::Priority::Highest, uuid);
            msg.push_uint(jobId);
            msg.push_string(uuid);
            msg.push_string(sBundle);
            msg.push_string(sFilePath);
            msg.send(cluster);

            {
                // Wait for the server to send back data, or in 10 seconds fail
                std::unique_lock<std::mutex> lock(fdObj.dataCVMutex);

                // Wait for data to be ready to send
                if (!fdObj.dataCV.wait_for(lock, std::chrono::seconds(30), [&fdObj] { return fdObj.dataReady; })) {
                    // Timeout reached, set the error
                    fdObj.error = true;
                    fdObj.errorDetails = "Client too took long to respond.";
                }
            }

            // Check if the server received an error about the file
            if (fdObj.error) {
                {
                    std::unique_lock<std::mutex> fileDownloadMapDeletionLock(fileDownloadMapDeletionLockMutex);

                    // Destroy the file download object
                    if (fileDownloadMap.find(uuid) != fileDownloadMap.end())
                        fileDownloadMap.erase(uuid);
                }

                response->write(SimpleWeb::StatusCode::client_error_bad_request, fdObj.errorDetails);
                return;
            }

            // Check if the server received details about the file
            if (!fdObj.receivedData) {
                {
                    std::unique_lock<std::mutex> fileDownloadMapDeletionLock(fileDownloadMapDeletionLockMutex);

                    // Destroy the file download object
                    if (fileDownloadMap.find(uuid) != fileDownloadMap.end())
                        fileDownloadMap.erase(uuid);
                }

                response->write(SimpleWeb::StatusCode::server_error_service_unavailable, "Remote Cluster Offline");
                return;
            }

            // Send the file size back to the client
            nlohmann::json result;
            result["fileId"] = uuid;

            SimpleWeb::CaseInsensitiveMultimap headers;
            headers.emplace("Content-Type", "application/octet-stream");

            // Get the filename from the download
            boost::filesystem::path fp(sFilePath);

            // Check if we need to tell the browser to force the download
            if (forceDownload)
                headers.emplace("Content-Disposition", "attachment; filename=\"" + fp.filename().string() + "\"");
            else
                headers.emplace("Content-Disposition", "filename=\"" + fp.filename().string() + "\"");

            // Set the content size
            headers.emplace("Content-Length", std::to_string(fdObj.fileSize));

            // Write the headers
            response->write(headers);
            std::promise<SimpleWeb::error_code> headerPromise;
            response->send([&headerPromise](const SimpleWeb::error_code &ec) {
                headerPromise.set_value(ec);
            });

            if (auto ec = headerPromise.get_future().get()) {
                throw std::runtime_error(
                        "Error transmitting file transfer headers to client. Perhaps client has disconnected? "
                        + std::to_string(ec.value()) + " " + ec.message()
                );
            }

            // Check for error, or all data sent
            while (!fdObj.error && fdObj.sentBytes < fdObj.fileSize) {
                {
                    // Wait for the server to send back data, or in 30 seconds fail
                    std::unique_lock<std::mutex> lock(fdObj.dataCVMutex);

                    // Wait for data to be ready to send
                    if (!fdObj.dataCV.wait_for(lock, std::chrono::seconds(30), [&fdObj] { return fdObj.dataReady; })) {
                        throw std::runtime_error("Client took too long to respond.");
                    }
                    fdObj.dataReady = false;
                }

                // While there is data in the queue, send to the client
                while (!fdObj.queue.empty()) {
                    // Get the next chunk from the queue
                    auto data = fdObj.queue.try_dequeue();

                    // If there was one, send it to the client
                    if (data) {
                        // Increment the traffic counter
                        fdObj.sentBytes += (*data)->size();

                        // Send the data
                        response->write((const char *) (*data)->data(), (std::streamsize) (*data)->size());

                        std::promise<SimpleWeb::error_code> contentPromise;
                        response->send([&contentPromise](const SimpleWeb::error_code &ec) {
                            contentPromise.set_value(ec);
                        });

                        if (auto ec = contentPromise.get_future().get()) {
                            throw std::runtime_error(
                                    "Error transmitting file content to client. Perhaps client has disconnected? "
                                    + std::to_string(ec.value()) + " " + ec.message());
                        }

                        // Clean up the data
                        delete (*data);

                        {
                            // The Pause/Resume messages must be synchronized to avoid a deadlock
                            std::unique_lock<std::mutex> fileDownloadPauseResumeLock(fileDownloadPauseResumeLockMutex);

                            // Check if we need to resume the client file transfer
                            if (fdObj.clientPaused) {
                                // Check if the buffer is smaller than the setting
                                if (fdObj.receivedBytes - fdObj.sentBytes < MIN_FILE_BUFFER_SIZE) {
                                    // Ask the client to resume the file transfer
                                    fdObj.clientPaused = false;

                                    auto resumeMsg = Message(RESUME_FILE_CHUNK_STREAM, Message::Priority::Highest,
                                                             uuid);
                                    resumeMsg.push_string(uuid);
                                    resumeMsg.send(cluster);
                                }
                            }
                        }
                    }
                }
            }

            {
                // Try to acquire the lock in case the cluster is still using it
                std::unique_lock<std::mutex> fileDownloadMapDeletionLock(fileDownloadMapDeletionLockMutex);

                // It's now safe to delete the uuid from the fileDownloadMap

                // Destroy the file download object
                if (fileDownloadMap.find(uuid) != fileDownloadMap.end())
                    fileDownloadMap.erase(uuid);

                // The lock can now be released, because when the lock is acquired in Cluster, the first thing it will do
                // is check for the existence of the uuid in the fileDownloadMap. But since we've removed it, Cluster is
                // guaranteed not to use the fdObj again.
            }
        } catch (std::exception& e) {
            dumpExceptions(e);

            {
                // Same as above
                std::unique_lock<std::mutex> fileDownloadMapDeletionLock(fileDownloadMapDeletionLockMutex);

                // Destroy the file download object
                if (fileDownloadMap.find(uuid) != fileDownloadMap.end())
                    fileDownloadMap.erase(uuid);

                // Same as above
            }

            // Report bad request
            response->write(SimpleWeb::StatusCode::client_error_bad_request, "Bad request");
        }
    };

    // List files in the specified directory
    server->getServer().resource["^" + path + "$"]["PATCH"] = [clusterManager, server](
            const std::shared_ptr<HttpServerImpl::Response> &response,
            const std::shared_ptr<HttpServerImpl::Request> &request) {

        // Verify that the user is authorized
        std::unique_ptr<sAuthorizationResult> authResult;
        try {
            authResult = server->isAuthorized(request->header);
        } catch (std::exception& e) {
            dumpExceptions(e);

            // Invalid request
            response->write(SimpleWeb::StatusCode::client_error_forbidden, "Not authorized");
            return;
        }

        try {
            // Create a vector which includes the application from the secret, and any other applications it has access to
            auto applications = std::vector<std::string>({authResult->secret().name()});
            std::copy(authResult->secret().applications().begin(), authResult->secret().applications().end(),
                      std::back_inserter(applications));

            // Read the json from the post body
            nlohmann::json post_data;
            request->content >> post_data;

            // Get the job to fetch files for
            auto jobId = (uint32_t) post_data["jobId"];

            // Get the job to fetch files for
            auto bRecursive = (bool) post_data["recursive"];

            // Get the path to the file to fetch (relative to the project)
            auto filePath = std::string(post_data["path"]);

            // Handle the file list request
            handleFileList(
                    clusterManager, jobId, bRecursive, filePath, authResult->secret().name(), applications,
                    response.get()
            );
        } catch (std::exception& e) {
            dumpExceptions(e);
            
            response->write(SimpleWeb::StatusCode::client_error_bad_request, "Bad request");
        }
    };
}