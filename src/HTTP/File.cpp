//
// Created by lewis on 3/12/20.
//

#include "../DB/MySqlConnector.h"
#include "../Cluster/ClusterManager.h"
#include "../Lib/jobserver_schema.h"
#include "HttpServer.h"
#include "Utils/HandleFileList.h"
#include <boost/filesystem.hpp>
#include <boost/lexical_cast.hpp>
#include <boost/uuid/random_generator.hpp>
#include <boost/uuid/uuid_io.hpp>


// NOLINTNEXTLINE(readability-function-cognitive-complexity)
void FileApi(const std::string &path, HttpServer *server, const std::shared_ptr<ClusterManager>& clusterManager) {
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
        auto database = MySqlConnector();

        // Start a transaction
        database->start_transaction();

        // Create a vector which includes the application from the secret, and any other applications it has access to
        auto applications = std::vector<std::string>({authResult->secret().name()});
        std::copy(authResult->secret().applications().begin(), authResult->secret().applications().end(),
                  std::back_inserter(applications));

        // Get the tables
        schema::JobserverJob jobTable;
        schema::JobserverFiledownload fileDownloadTable;

        try {
            // Read the json from the post body
            nlohmann::json post_data;
            request->content >> post_data;

            // Get the job to fetch files for if one was provided
            auto jobId = post_data.contains("jobId") ? static_cast<uint32_t>(post_data["jobId"]) : 0;

            // Get the path to the file to fetch (relative to the project)
            bool hasPaths = false;
            std::vector<std::string> filePaths;
            if (post_data.contains("paths")) {
                filePaths = post_data["paths"].get<std::vector<std::string>>();
                hasPaths = true;
            } else {
                filePaths.push_back(std::string{post_data["path"]});
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

            auto sCluster = std::string{};
            auto sBundle = std::string{};

            // If a job ID was provided, fetch the cluster and bundle details from that job
            if (jobId != 0) {
                // Look up the job
                auto jobResults =
                        database->run(
                                select(all_of(jobTable))
                                        .from(jobTable)
                                        .where(
                                                jobTable.id == static_cast<uint32_t>(jobId)
                                                and jobTable.application.in(sqlpp::value_list(applications))
                                        )
                        );

                // Check that a job was actually found
                if (jobResults.empty()) {
                    throw std::runtime_error(
                            "Unable to find job with ID " + std::to_string(jobId) + " for application " +
                            authResult->secret().name());
                }

                // Get the cluster and bundle from the job
                const auto *job = &jobResults.front();
                sCluster = std::string{job->cluster};
                sBundle = std::string{job->bundle};
            } else {
                // A job ID was not provided, we need to use the cluster and bundle details from the json request
                // Check that the bundle and cluster were provided in the POST json
                if (!post_data.contains("cluster") || !post_data.contains("bundle")) {
                    throw std::runtime_error(
                            "The 'cluster' and 'bundle' parameters were not provided in the absence of 'jobId'"
                            );
                }

                sCluster = std::string{post_data["cluster"]};
                sBundle = std::string{post_data["bundle"]};

                // Confirm that the current JWT secret can access the provided cluster
                const auto& clusters = authResult->secret().clusters();
                if (std::find(clusters.begin(), clusters.end(), sCluster) == clusters.end()) {
                    throw std::runtime_error(
                            "Application " + authResult->secret().name() + " does not have access to cluster " + sCluster
                            );
                }
            }

            // Create a multi insert query
            auto insert_query = insert_into(fileDownloadTable).columns(
                    fileDownloadTable.user,
                    fileDownloadTable.job,
                    fileDownloadTable.cluster,
                    fileDownloadTable.bundle,
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
                        fileDownloadTable.user = static_cast<int>(authResult->payload()["userId"]),
                        fileDownloadTable.job = static_cast<int>(jobId),
                        fileDownloadTable.cluster = sCluster,
                        fileDownloadTable.bundle = sBundle,
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
                database->run(insert_query);

                // Commit the changes in the database
                database->commit_transaction();
            } catch (sqlpp::exception &e) {
                dumpExceptions(e);

                // Uh oh, an error occurred
                // Abort the transaction
                database->rollback_transaction(false);

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
            database->rollback_transaction(false);

            // Report bad request
            response->write(SimpleWeb::StatusCode::client_error_bad_request, "Bad request");
        }
    };

    // Download file
    // NOLINTNEXTLINE(readability-function-cognitive-complexity)
    server->getServer().resource["^" + path + "$"]["GET"] = [clusterManager](
            const std::shared_ptr<HttpServerImpl::Response> &response,
            const std::shared_ptr<HttpServerImpl::Request> &request) {

        // Create a database connection
        auto database = MySqlConnector();

        // Get the tables
        schema::JobserverJob jobTable;
        schema::JobserverFiledownload fileDownloadTable;

        // Create a new file download object
        auto fdObj = std::make_shared<sFileDownload>();
        std::string uuid;

        try {
            // Process the query parameters
            auto query_fields = request->parse_query_string();

            // Check if jobId is provided
            auto uuidPtr = query_fields.find("fileId");
            if (uuidPtr != query_fields.end()) {
                uuid = uuidPtr->second;
            }

            if (uuid.empty()) {
                response->write(SimpleWeb::StatusCode::client_error_bad_request, "Bad Request");
                return;
            }

            // Check if forceDownload is provided
            auto fdPtr = query_fields.find("forceDownload");
            bool forceDownload = false;
            if (fdPtr != query_fields.end()) {
                forceDownload = true;
            }

            // Expire any old file downloads
            database->run(
                    remove_from(fileDownloadTable)
                            .where(
                                    fileDownloadTable.timestamp <=
                                    std::chrono::system_clock::now() -
                                    std::chrono::seconds(FILE_DOWNLOAD_EXPIRY_TIME)
                            )

            );

            // Look up the file download
            auto dlResults = database->run(
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
            const auto *dlResult = &dlResults.front();

            auto sCluster = std::string{dlResult->cluster};
            auto sBundle = std::string{dlResult->bundle};

            auto sFilePath = std::string{dlResult->path};
            auto jobId = static_cast<uint32_t>(dlResult->job);

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

            // Generate a new uuid so that simultanious downloads of the same file don't cause a collision between server->client
            // Refer to https://phab.adacs.org.au/D665 for additonal details and justification
            uuid = boost::lexical_cast<std::string>(boost::uuids::random_generator()());

            // Cluster is online, create the file hash object
            fileDownloadMap->emplace(uuid, fdObj);

            // Send a message to the client to initiate the file download
            auto msg = Message(DOWNLOAD_FILE, Message::Priority::Highest, uuid);
            msg.push_uint(jobId);
            msg.push_string(uuid);
            msg.push_string(sBundle);
            msg.push_string(sFilePath);
            msg.send(cluster);

            {
                // Wait for the server to send back data, or in CLIENT_TIMEOUT_SECONDS fail
                std::unique_lock<std::mutex> lock(fdObj->dataCVMutex);

                // Wait for data to be ready to send
                if (!fdObj->dataCV.wait_for(lock, std::chrono::seconds(CLIENT_TIMEOUT_SECONDS), [&fdObj] { return fdObj->dataReady; })) {
                    // Timeout reached, set the error
                    fdObj->error = true;
                    fdObj->errorDetails = "Client too took long to respond.";
                }
            }

            // Check if the server received an error about the file
            if (fdObj->error) {
                {
                    std::unique_lock<std::mutex> fileDownloadMapDeletionLock(fileDownloadMapDeletionLockMutex);

                    // Destroy the file download object
                    if (fileDownloadMap->find(uuid) != fileDownloadMap->end()) {
                        fileDownloadMap->erase(uuid);
                    }
                }

                response->write(SimpleWeb::StatusCode::client_error_bad_request, fdObj->errorDetails);
                return;
            }

            // Check if the server received details about the file
            if (!fdObj->receivedData) {
                {
                    std::unique_lock<std::mutex> fileDownloadMapDeletionLock(fileDownloadMapDeletionLockMutex);

                    // Destroy the file download object
                    if (fileDownloadMap->find(uuid) != fileDownloadMap->end()) {
                        fileDownloadMap->erase(uuid);
                    }
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
            boost::filesystem::path filePath(sFilePath);

            // Check if we need to tell the browser to force the download
            if (forceDownload) {
                headers.emplace("Content-Disposition", "attachment; filename=\"" + filePath.filename().string() + "\"");
            } else {
                headers.emplace("Content-Disposition", "filename=\"" + filePath.filename().string() + "\"");
            }

            // Set the content size
            headers.emplace("Content-Length", std::to_string(fdObj->fileSize));

            // Write the headers
            response->write(headers);
            std::promise<SimpleWeb::error_code> headerPromise;
            response->send([&headerPromise](const SimpleWeb::error_code &errorCode) {
                headerPromise.set_value(errorCode);
            });

            if (auto errorCode = headerPromise.get_future().get()) {
                throw std::runtime_error(
                        "Error transmitting file transfer headers to client. Perhaps client has disconnected? "
                        + std::to_string(errorCode.value()) + " " + errorCode.message()
                );
            }

            // Check for error, or all data sent
            while (!fdObj->error && fdObj->sentBytes < fdObj->fileSize) {
                {
                    // Wait for the server to send back data, or in CLIENT_TIMEOUT_SECONDS fail
                    std::unique_lock<std::mutex> lock(fdObj->dataCVMutex);

                    // Wait for data to be ready to send
                    if (!fdObj->dataCV.wait_for(lock, std::chrono::seconds(CLIENT_TIMEOUT_SECONDS), [&fdObj] { return fdObj->dataReady; })) {
                        throw std::runtime_error("Client took too long to respond.");
                    }
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
                        // NOLINTNEXTLINE(cppcoreguidelines-pro-type-reinterpret-cast)
                        response->write(reinterpret_cast<const char *>((*data)->data()), static_cast<std::streamsize>((*data)->size()));

                        std::promise<SimpleWeb::error_code> contentPromise;
                        response->send([&contentPromise](const SimpleWeb::error_code &errorCode) {
                            contentPromise.set_value(errorCode);
                        });

                        if (auto errorCode = contentPromise.get_future().get()) {
                            throw std::runtime_error(
                                    "Error transmitting file content to client. Perhaps client has disconnected? "
                                    + std::to_string(errorCode.value()) + " " + errorCode.message());
                        }

                        {
                            // The Pause/Resume messages must be synchronized to avoid a deadlock
                            std::unique_lock<std::mutex> fileDownloadPauseResumeLock(fileDownloadPauseResumeLockMutex);

                            // Check if we need to resume the client file transfer
                            if (fdObj->clientPaused) {
                                // Check if the buffer is smaller than the setting
                                if (fdObj->receivedBytes - fdObj->sentBytes < MIN_FILE_BUFFER_SIZE) {
                                    // Ask the client to resume the file transfer
                                    fdObj->clientPaused = false;

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
                if (fileDownloadMap->find(uuid) != fileDownloadMap->end()) {
                    fileDownloadMap->erase(uuid);
                }

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
                if (fileDownloadMap->find(uuid) != fileDownloadMap->end()) {
                    fileDownloadMap->erase(uuid);
                }

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

            // Get the job to fetch files for if one was provided
            auto jobId = post_data.contains("jobId") ? static_cast<uint32_t>(post_data["jobId"]) : 0;

            // Get the job to fetch files for
            auto bRecursive = static_cast<bool>(post_data["recursive"]);

            // Get the path to the file to fetch (relative to the project)
            auto filePath = std::string{post_data["path"]};

            if (jobId != 0) {
                // Handle the file list request
                handleFileList(
                        clusterManager, jobId, bRecursive, filePath, authResult->secret().name(), applications,
                        response
                );
            } else {
                // A job ID was not provided, we need to use the cluster and bundle details from the json request
                // Check that the bundle and cluster were provided in the POST json
                if (!post_data.contains("cluster") || !post_data.contains("bundle")) {
                    throw std::runtime_error(
                            "The 'cluster' and 'bundle' parameters were not provided in the absence of 'jobId'"
                    );
                }

                auto sCluster = std::string{post_data["cluster"]};
                auto sBundle = std::string{post_data["bundle"]};

                // Get the cluster to submit to
                auto cluster = clusterManager->getCluster(sCluster);
                if (!cluster) {
                    // Invalid cluster
                    throw std::runtime_error("Invalid cluster");
                }

                // Confirm that the current JWT secret can access the provided cluster
                const auto& clusters = authResult->secret().clusters();
                if (std::find(clusters.begin(), clusters.end(), sCluster) == clusters.end()) {
                    throw std::runtime_error(
                            "Application " + authResult->secret().name() + " does not have access to cluster " + sCluster
                    );
                }

                // Handle the file list request
                handleFileList(
                        cluster, sBundle, 0, bRecursive, filePath, response
                );
            }
        } catch (std::exception& exception) {
            dumpExceptions(exception);
            
            response->write(SimpleWeb::StatusCode::client_error_bad_request, "Bad request");
        }
    };
}
