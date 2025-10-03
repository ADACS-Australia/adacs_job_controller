//
// Created by lewis on 3/12/20.
//

module;
#include <algorithm>
#include <chrono>
#include <future>
#include <iomanip>
#include <ostream>

#include <asio_compatibility.hpp>
#include <boost/filesystem.hpp>
#include <boost/lexical_cast.hpp>
#include <boost/uuid/random_generator.hpp>
#include <boost/uuid/uuid_io.hpp>
#include <nlohmann/json.hpp>
#include <sqlpp11/sqlpp11.h>
#include <status_code.hpp>
#include <utility.hpp>

#include "../Lib/shims/date_shim.h"

import MySqlConnector;
import IClusterManager;
import ICluster;
import FileDownload;
import FileUpload;
import jobserver_schema;
import Message;
import settings;
import IApplication;
import HttpServer;
import HandleFileList;
import GeneralUtils;

export module File;

export void FileApi(const std::string& path,
                    const std::shared_ptr<HttpServer>& server,
                    const std::shared_ptr<IApplication>& app)
{
    // Get      -> Download file (file uuid)
    // Post     -> Create new file download
    // Delete   -> Delete file download (file uuid)
    // Patch    -> List files in directory

    auto clusterManager = app->getClusterManager();

    // Create a new file download
    server->getServer().resource["^" + path + "$"]["POST"] =
        [server](const std::shared_ptr<HttpServerImpl::Response>& response,
                 const std::shared_ptr<HttpServerImpl::Request>& request) {
            // Verify that the user is authorized
            std::unique_ptr<sAuthorizationResult> authResult;
            try
            {
                authResult = server->isAuthorized(request->header);
            }
            catch (std::exception& e)
            {
                dumpExceptions(e);

                // Invalid request
                response->write(SimpleWeb::StatusCode::client_error_forbidden, "Not authorized");
                return;
            }

            // Create a database connection
            auto database = MySqlConnector();

            // Start a transaction
            database->start_transaction();

            // Create a vector which includes the application from the secret, and any other applications it has access
            // to
            auto applications = std::vector<std::string>({authResult->secret().name()});
            std::copy(authResult->secret().applications().begin(),
                      authResult->secret().applications().end(),
                      std::back_inserter(applications));

            // Get the tables
            const schema::JobserverJob jobTable;
            const schema::JobserverFiledownload fileDownloadTable;

            try
            {
                // Read the json from the post body
                nlohmann::json post_data;
                request->content >> post_data;

                // NOLINTBEGIN(cppcoreguidelines-pro-bounds-avoid-unchecked-container-access)
                // Get the job to fetch files for if one was provided
                auto jobId = post_data.contains("jobId") ? static_cast<uint64_t>(post_data["jobId"]) : 0;

                // Get the path to the file to fetch (relative to the project)
                bool hasPaths = false;
                std::vector<std::string> filePaths;
                if (post_data.contains("paths"))
                {
                    filePaths = post_data["paths"].get<std::vector<std::string>>();
                    hasPaths  = true;
                }
                else
                {
                    filePaths.push_back(std::string{post_data["path"]});
                }
                // NOLINTEND(cppcoreguidelines-pro-bounds-avoid-unchecked-container-access)

                // Check that there were actually file paths provided
                if (filePaths.empty())
                {
                    // Return an empty result, this is only possible when using "paths"
                    nlohmann::json result;
                    // NOLINTNEXTLINE(cppcoreguidelines-pro-bounds-avoid-unchecked-container-access)
                    result["fileIds"] = std::vector<std::string>();

                    SimpleWeb::CaseInsensitiveMultimap headers;
                    headers.emplace("Content-Type", "application/json");

                    response->write(SimpleWeb::StatusCode::success_ok, result.dump(), headers);
                    return;
                }

                auto sCluster = std::string{};
                auto sBundle  = std::string{};

                // If a job ID was provided, fetch the cluster and bundle details from that job
                if (jobId != 0)
                {
                    // Look up the job
                    auto jobResults = database->run(
                        select(all_of(jobTable))
                            .from(jobTable)
                            .where(jobTable.id == jobId and jobTable.application.in(sqlpp::value_list(applications))));

                    // Check that a job was actually found
                    if (jobResults.empty())
                    {
                        throw std::runtime_error("Unable to find job with ID " + std::to_string(jobId) +
                                                 " for application " + authResult->secret().name());
                    }

                    // Get the cluster and bundle from the job
                    const auto* job = &jobResults.front();
                    sCluster        = std::string{job->cluster};
                    sBundle         = std::string{job->bundle};
                }
                else
                {
                    // A job ID was not provided, we need to use the cluster and bundle details from the json request
                    // Check that the bundle and cluster were provided in the POST json
                    if (!post_data.contains("cluster") || !post_data.contains("bundle"))
                    {
                        throw std::runtime_error(
                            "The 'cluster' and 'bundle' parameters were not provided in the absence of 'jobId'");
                    }

                    // NOLINTBEGIN(cppcoreguidelines-pro-bounds-avoid-unchecked-container-access)
                    sCluster = std::string{post_data["cluster"]};
                    sBundle  = std::string{post_data["bundle"]};
                    // NOLINTEND(cppcoreguidelines-pro-bounds-avoid-unchecked-container-access)

                    // Confirm that the current JWT secret can access the provided cluster
                    const auto& clusters = authResult->secret().clusters();
                    if (std::ranges::find(clusters, sCluster) == clusters.end())
                    {
                        throw std::runtime_error("Application " + authResult->secret().name() +
                                                 " does not have access to cluster " + sCluster);
                    }
                }

                // Create a multi insert query
                auto insert_query = insert_into(fileDownloadTable)
                                        .columns(fileDownloadTable.user,
                                                 fileDownloadTable.job,
                                                 fileDownloadTable.cluster,
                                                 fileDownloadTable.bundle,
                                                 fileDownloadTable.uuid,
                                                 fileDownloadTable.path,
                                                 fileDownloadTable.timestamp);

                // Now iterate over the file paths and generate UUID's for them
                std::vector<std::string> uuids;
                for (const auto& path : filePaths)
                {
                    // Generate a UUID for the download
                    auto uuid = boost::lexical_cast<std::string>(boost::uuids::random_generator()());

                    // Save the path
                    uuids.push_back(uuid);

                    // Add the record to be inserted
                    // NOLINTBEGIN(cppcoreguidelines-pro-bounds-avoid-unchecked-container-access)
                    insert_query.values.add(
                        fileDownloadTable.user    = static_cast<int>(authResult->payload()["userId"]),
                        fileDownloadTable.job     = static_cast<int>(jobId),
                        fileDownloadTable.cluster = sCluster,
                        fileDownloadTable.bundle  = sBundle,
                        fileDownloadTable.uuid    = uuid,
                        fileDownloadTable.path    = std::string(path),
                        fileDownloadTable.timestamp =
                            std::chrono::time_point_cast<std::chrono::microseconds>(std::chrono::system_clock::now()));
                    // NOLINTEND(cppcoreguidelines-pro-bounds-avoid-unchecked-container-access)
                }

                // Try inserting the values in the database
                try
                {
                    [[maybe_unused]] auto insertResult = database->run(insert_query);

                    // Commit the changes in the database
                    database->commit_transaction();
                }
                catch (sqlpp::exception& e)
                {
                    dumpExceptions(e);

                    // Uh oh, an error occurred
                    // Abort the transaction
                    database->rollback_transaction(false);

                    // Report bad request
                    response->write(SimpleWeb::StatusCode::client_error_bad_request,
                                    "Unable to insert records in the database, please try again later");
                    return;
                }

                // Report success
                nlohmann::json result;
                // NOLINTBEGIN(cppcoreguidelines-pro-bounds-avoid-unchecked-container-access)
                if (hasPaths)
                {
                    result["fileIds"] = uuids;
                }
                else
                {
                    result["fileId"] = uuids[0];
                }
                // NOLINTEND(cppcoreguidelines-pro-bounds-avoid-unchecked-container-access)

                SimpleWeb::CaseInsensitiveMultimap headers;
                headers.emplace("Content-Type", "application/json");

                response->write(SimpleWeb::StatusCode::success_ok, result.dump(), headers);
            }
            catch (std::exception& e)
            {
                dumpExceptions(e);

                // Abort the transaction
                database->rollback_transaction(false);

                // Report bad request
                response->write(SimpleWeb::StatusCode::client_error_bad_request, "Bad request");
            }
        };

    // Download file
    server->getServer()
        .resource["^" + path + "$"]["GET"] = [clusterManager,
                                              app](const std::shared_ptr<HttpServerImpl::Response>& response,
                                                   const std::shared_ptr<HttpServerImpl::Request>& request) {
        // Create a database connection
        auto database = MySqlConnector();

        // Get the tables
        const schema::JobserverJob jobTable;
        const schema::JobserverFiledownload fileDownloadTable;

        // Create a new file download object
        std::string uuid;
        std::shared_ptr<FileDownload> fdObj;

        try
        {
            // Process the query parameters
            auto query_fields = request->parse_query_string();

            // Check if jobId is provided
            auto uuidPtr = query_fields.find("fileId");
            if (uuidPtr != query_fields.end())
            {
                uuid = uuidPtr->second;
            }

            if (uuid.empty())
            {
                response->write(SimpleWeb::StatusCode::client_error_bad_request, "Bad Request");
                return;
            }

            // Check if forceDownload is provided
            auto fdPtr         = query_fields.find("forceDownload");
            bool forceDownload = false;
            if (fdPtr != query_fields.end())
            {
                forceDownload = true;
            }

            // Expire any old file downloads
            // NOLINTNEXTLINE(bugprone-unused-return-value)
            database->run(
                remove_from(fileDownloadTable)
                    .where(fileDownloadTable.timestamp <=
                           std::chrono::system_clock::now() - std::chrono::seconds(FILE_DOWNLOAD_EXPIRY_TIME)));

            // Look up the file download
            auto dlResults = database->run(
                select(all_of(fileDownloadTable)).from(fileDownloadTable).where(fileDownloadTable.uuid == uuid));

            // Check that the uuid existed in the database
            if (dlResults.empty())
            {
                response->write(SimpleWeb::StatusCode::client_error_bad_request, "Bad Request");
                return;
            }

            // Get the cluster and bundle from the job
            const auto* dlResult = &dlResults.front();

            auto sCluster = std::string{dlResult->cluster};
            auto sBundle  = std::string{dlResult->bundle};

            auto sFilePath = std::string{dlResult->path};
            auto jobId     = static_cast<uint64_t>(dlResult->job);

            // Check that the cluster is online
            // Get the cluster to submit to
            auto cluster = clusterManager->getCluster(sCluster);
            if (!cluster)
            {
                // Invalid cluster
                throw std::runtime_error("Invalid cluster");
            }

            // If the cluster isn't online then there isn't anything to do
            if (!cluster->isOnline())
            {
                response->write(SimpleWeb::StatusCode::server_error_service_unavailable, "Remote Cluster Offline");
                return;
            }

            // Generate a new uuid so that simultanious downloads of the same file don't cause a collision between
            // server->client Refer to https://phab.adacs.org.au/D665 for additonal details and justification
            uuid = boost::lexical_cast<std::string>(boost::uuids::random_generator()());

            // Create the file download object
            fdObj = std::static_pointer_cast<FileDownload>(clusterManager->createFileDownload(cluster, uuid));

            // Send a message to the client to initiate the file download
            auto msg = Message(DOWNLOAD_FILE, Message::Priority::Highest, uuid);
            msg.push_uint(jobId);
            msg.push_string(uuid);
            msg.push_string(sBundle);
            msg.push_string(sFilePath);
            cluster->sendMessage(msg);

            {
                // Wait for the server to send back data, or in CLIENT_TIMEOUT_SECONDS fail
                std::unique_lock<std::mutex> lock(fdObj->fileDownloadDataCVMutex);

                // Wait for data to be ready to send
                if (!fdObj->fileDownloadDataCV.wait_for(lock, std::chrono::seconds(CLIENT_TIMEOUT_SECONDS), [&fdObj] {
                        return fdObj->fileDownloadDataReady;
                    }))
                {
                    // Timeout reached, set the error
                    fdObj->fileDownloadError        = true;
                    fdObj->fileDownloadErrorDetails = "Client took too long to respond.";
                }
            }

            // Check if the server received an error about the file
            if (fdObj->fileDownloadError)
            {
                response->write(SimpleWeb::StatusCode::client_error_bad_request, fdObj->fileDownloadErrorDetails);
                return;
            }

            // Check if the server received details about the file
            if (!fdObj->fileDownloadReceivedData)
            {
                response->write(SimpleWeb::StatusCode::server_error_service_unavailable, "Remote Cluster Offline");
                return;
            }

            // Send the file size back to the client
            nlohmann::json result;
            // NOLINTNEXTLINE(cppcoreguidelines-pro-bounds-avoid-unchecked-container-access)
            result["fileId"] = uuid;

            SimpleWeb::CaseInsensitiveMultimap headers;
            headers.emplace("Content-Type", "application/octet-stream");

            // Get the filename from the download
            const boost::filesystem::path filePath(sFilePath);

            // Check if we need to tell the browser to force the download
            if (forceDownload)
            {
                headers.emplace("Content-Disposition", "attachment; filename=\"" + filePath.filename().string() + "\"");
            }
            else
            {
                headers.emplace("Content-Disposition", "filename=\"" + filePath.filename().string() + "\"");
            }

            // Set the content size
            headers.emplace("Content-Length", std::to_string(fdObj->fileDownloadFileSize));

            // Write the headers
            response->write(headers);
            std::promise<SimpleWeb::error_code> headerPromise;
            response->send([&headerPromise](const SimpleWeb::error_code& errorCode) {
                headerPromise.set_value(errorCode);
            });

            if (auto errorCode = headerPromise.get_future().get())
            {
                throw std::runtime_error(
                    "Error transmitting file transfer headers to client. Perhaps client has disconnected? " +
                    std::to_string(errorCode.value()) + " " + errorCode.message());
            }

            // Check for error, or all data sent
            while (!fdObj->fileDownloadError && fdObj->fileDownloadSentBytes < fdObj->fileDownloadFileSize)
            {
                {
                    // Wait for the server to send back data, or in CLIENT_TIMEOUT_SECONDS fail
                    std::unique_lock<std::mutex> lock(fdObj->fileDownloadDataCVMutex);

                    // Wait for data to be ready to send
                    if (!fdObj->fileDownloadDataCV.wait_for(lock,
                                                            std::chrono::seconds(CLIENT_TIMEOUT_SECONDS),
                                                            [&fdObj] {
                                                                return fdObj->fileDownloadDataReady;
                                                            }))
                    {
                        throw std::runtime_error("Client took too long to respond.");
                    }
                    fdObj->fileDownloadDataReady = false;
                }

                // While there is data in the queue, send to the client
                while (!fdObj->fileDownloadQueue.empty())
                {
                    // Get the next chunk from the queue
                    auto data = fdObj->fileDownloadQueue.try_dequeue();

                    // If there was one, send it to the client
                    if (data)
                    {
                        // Increment the traffic counter
                        fdObj->fileDownloadSentBytes += (*data)->size();

                        // Send the data
                        // NOLINTNEXTLINE(cppcoreguidelines-pro-type-reinterpret-cast)
                        response->write(reinterpret_cast<const char*>((*data)->data()),
                                        static_cast<std::streamsize>((*data)->size()));

                        std::promise<SimpleWeb::error_code> contentPromise;
                        response->send([&contentPromise](const SimpleWeb::error_code& errorCode) {
                            contentPromise.set_value(errorCode);
                        });

                        if (auto errorCode = contentPromise.get_future().get())
                        {
                            throw std::runtime_error(
                                "Error transmitting file content to client. Perhaps client has disconnected? " +
                                std::to_string(errorCode.value()) + " " + errorCode.message());
                        }

                        {
                            // The Pause/Resume messages must be synchronized to avoid a deadlock
                            const std::unique_lock<std::mutex> fileDownloadPauseResumeLock(
                                app->getFileDownloadPauseResumeLockMutex());

                            // Check if we need to resume the client file transfer
                            if (fdObj->fileDownloadClientPaused)
                            {
                                // Check if the buffer is smaller than the setting
                                if (fdObj->fileDownloadReceivedBytes - fdObj->fileDownloadSentBytes <
                                    MIN_FILE_BUFFER_SIZE)
                                {
                                    // Ask the client to resume the file transfer
                                    fdObj->fileDownloadClientPaused = false;

                                    auto resumeMsg =
                                        Message(RESUME_FILE_CHUNK_STREAM, Message::Priority::Highest, uuid);
                                    fdObj->sendMessage(resumeMsg);
                                }
                            }
                        }
                    }
                }
            }

            fdObj->close(false);
        }
        catch (std::exception& e)
        {
            dumpExceptions(e);

            if (fdObj)
            {
                // Close the connection prematurely.
                fdObj->close(true);
            }

            // If the response is closed, and we try to send more data, it will raise an exception - so wrap this in a
            // try/catch
            try
            {
                // Report bad request
                response->close_connection_after_response = true;
                response->write(SimpleWeb::StatusCode::client_error_bad_request, "Bad request");
            }
            catch (std::exception& e)
            {
                // Ignore exceptions during connection cleanup
                (void)e;
            }
        }
    };

    // ========================================================================
    // File Upload Endpoint: PUT /v1/file/upload/
    // ========================================================================
    // Streams file data from HTTP client to remote cluster via WebSocket.
    // Uses a two-connection architecture:
    //   1. HTTP connection (this handler) - receives file data from client
    //   2. WebSocket connection (FileUpload) - forwards chunks to remote cluster
    //
    // Algorithm:
    //   1. Authenticate request and validate cluster/bundle access
    //   2. Create FileUpload object and send UPLOAD_FILE message to remote
    //   3. Wait for remote to acknowledge (SERVER_READY message)
    //   4. Stream file in chunks with backpressure management:
    //      - Read chunk from HTTP request stream
    //      - Wait if WebSocket queue is too full (>MAX_FILE_BUFFER_SIZE)
    //      - Send chunk via WebSocket (at Lowest priority)
    //      - Repeat until all data sent
    //   5. Wait for queue to fully drain
    //   6. Send FILE_UPLOAD_COMPLETE message (at Highest priority)
    //   7. Wait for remote confirmation, return success or error to HTTP client
    //
    // Thread coordination:
    //   - HTTP thread (this): Reads from request.content, pushes to WebSocket queue
    //   - WebSocket thread: Consumes queue, sends to remote, handles responses
    //   - Synchronization via FileUpload condition variables and atomic queue size
    //
    // Backpressure:
    //   - Prevents memory exhaustion by pausing HTTP reads when queue is too full
    //   - Uses hysteresis: pause at MAX threshold, resume at MIN threshold
    //   - Intentionally blocks HTTP thread (client must wait) rather than buffering
    // ========================================================================
    server->getServer().resource["^" + path + "upload/$"]["PUT"] =
        [server, clusterManager](const std::shared_ptr<HttpServerImpl::Response>& response,
                                 const std::shared_ptr<HttpServerImpl::Request>& request) {
            // Helper lambda to send error responses consistently
            auto sendError = [&response](SimpleWeb::StatusCode status, const std::string& errorMessage) {
                nlohmann::json error_result;
                error_result.emplace("error", errorMessage);
                SimpleWeb::CaseInsensitiveMultimap headers;
                headers.emplace("Content-Type", "application/json");
                response->write(status, error_result.dump(), headers);
            };

            // Verify that the user is authorized
            std::unique_ptr<sAuthorizationResult> authResult;
            try
            {
                authResult = server->isAuthorized(request->header);
            }
            catch (std::exception& e)
            {
                dumpExceptions(e);

                // Invalid request
                response->write(SimpleWeb::StatusCode::client_error_forbidden, "Not authorized");
                return;
            }

            try
            {
                // ============================================================
                // Step 1: Parse request parameters and validate authorization
                // ============================================================

                // Parse query parameters for job ID and target path
                auto query_fields = request->parse_query_string();

                // Get the job to upload files for if one was provided
                // If jobId is provided, we'll look up cluster/bundle from the job
                // and verify the JWT token has access to that job's application
                auto jobId    = uint64_t{0};
                auto jobIdPtr = query_fields.find("jobId");
                if (jobIdPtr != query_fields.end())
                {
                    jobId = std::stoull(jobIdPtr->second);
                }

                // Get the target path for the file upload
                // Note: Path validation happens on the remote cluster side (it knows its filesystem)
                auto targetPathPtr = query_fields.find("targetPath");
                if (targetPathPtr == query_fields.end())
                {
                    sendError(SimpleWeb::StatusCode::client_error_bad_request, "targetPath parameter is required");
                    return;
                }
                auto targetPath = std::string{targetPathPtr->second};

                // Get file size from Content-Length header
                // This is required to know when we've read all the data and to send to remote
                auto contentLength = request->header.find("Content-Length");
                if (contentLength == request->header.end())
                {
                    sendError(SimpleWeb::StatusCode::client_error_bad_request, "Content-Length header is required");
                    return;
                }
                const uint64_t fileSize = std::stoull(contentLength->second);

                auto sCluster = std::string{};
                auto sBundle  = std::string{};

                // Determine cluster and bundle - either from jobId lookup or direct parameters
                // This also verifies that the authenticated JWT token has access to the target
                if (jobId != 0)
                {
                    // Route A: Look up cluster/bundle from job ID
                    // Verify JWT token has access to the job's application

                    // Create a database connection
                    auto database = MySqlConnector();

                    // Get the tables
                    const schema::JobserverJob jobTable;

                    // Look up the job, filtering by applications the JWT token can access
                    // This ensures users can only upload to jobs they own or have access to
                    auto applications = std::vector<std::string>({authResult->secret().name()});
                    std::copy(authResult->secret().applications().begin(),
                              authResult->secret().applications().end(),
                              std::back_inserter(applications));

                    auto jobResults = database->run(
                        select(all_of(jobTable))
                            .from(jobTable)
                            .where(jobTable.id == jobId and jobTable.application.in(sqlpp::value_list(applications))));

                    // Check that a job was actually found
                    if (jobResults.empty())
                    {
                        throw std::runtime_error("Unable to find job with ID " + std::to_string(jobId) +
                                                 " for application " + authResult->secret().name());
                    }

                    // Get the cluster and bundle from the job
                    const auto* job = &jobResults.front();
                    sCluster        = std::string{job->cluster};
                    sBundle         = std::string{job->bundle};
                }
                else
                {
                    // Route B: Use cluster/bundle from query parameters directly
                    // Verify JWT token has access to the specified cluster

                    // A job ID was not provided, we need to use the cluster and bundle details from query parameters
                    // Check that the bundle and cluster were provided in query parameters
                    auto clusterPtr = query_fields.find("cluster");
                    auto bundlePtr  = query_fields.find("bundle");
                    if (clusterPtr == query_fields.end() || bundlePtr == query_fields.end())
                    {
                        throw std::runtime_error(
                            "The 'cluster' and 'bundle' parameters were not provided in the absence of 'jobId'");
                    }

                    sCluster = std::string{clusterPtr->second};
                    sBundle  = std::string{bundlePtr->second};

                    // Confirm that the current JWT secret can access the provided cluster
                    // This prevents users from uploading to arbitrary clusters they don't own
                    const auto& clusters = authResult->secret().clusters();
                    if (std::ranges::find(clusters, sCluster) == clusters.end())
                    {
                        throw std::runtime_error("Application " + authResult->secret().name() +
                                                 " does not have access to cluster " + sCluster);
                    }
                }

                // ============================================================
                // Step 2: Verify cluster is online and initiate upload session
                // ============================================================

                // Check that the cluster is online
                auto cluster = clusterManager->getCluster(sCluster);
                if (!cluster)
                {
                    throw std::runtime_error("Invalid cluster");
                }

                if (!cluster->isOnline())
                {
                    sendError(SimpleWeb::StatusCode::server_error_service_unavailable, "Remote cluster is offline");
                    return;
                }

                // Generate a UUID for the upload session
                // This UUID identifies the upload across both HTTP and WebSocket connections
                auto uuid = boost::lexical_cast<std::string>(boost::uuids::random_generator()());

                // Create the FileUpload object (WebSocket sink side)
                // This registers the upload session so when the remote cluster connects back
                // via WebSocket with this UUID, it will be routed to this FileUpload object
                auto fileUpload = std::static_pointer_cast<FileUpload>(clusterManager->createFileUpload(cluster, uuid));

                // Send UPLOAD_FILE message to remote cluster via existing WebSocket connection
                // This tells the remote: "Expect a file upload session with this UUID, jobId, bundle, path, and size"
                // The remote will validate the path and working directory, then send back either SERVER_READY or FILE_UPLOAD_ERROR
                // Message format follows DOWNLOAD_FILE pattern for consistency
                auto msg = Message(UPLOAD_FILE, Message::Priority::Highest, uuid);
                msg.push_uint(jobId);         // Job ID (0 if no job, use bundle)
                msg.push_string(sBundle);     // Bundle hash for working directory resolution
                msg.push_string(targetPath);  // Target file path
                msg.push_ulong(fileSize);     // Expected file size
                cluster->sendMessage(msg);

                // ============================================================
                // Step 3: Wait for remote cluster to acknowledge upload request
                // ============================================================

                // Wait for the remote cluster to respond with SERVER_READY or FILE_UPLOAD_ERROR
                // The remote validates the target path and prepares to receive chunks
                {
                    std::unique_lock<std::mutex> lock(fileUpload->fileUploadDataCVMutex);

                    // Wait for data to be ready to send (signaled by FileUpload::handleMessage)
                    // Timeout prevents indefinite hang if remote doesn't respond
                    if (!fileUpload->fileUploadDataCV.wait_for(lock,
                                                               std::chrono::seconds(CLIENT_TIMEOUT_SECONDS),
                                                               [&fileUpload] {
                                                                   return fileUpload->fileUploadDataReady;
                                                               }))
                    {
                        // Timeout reached - remote didn't respond in time, abort upload
                        fileUpload->fileUploadError        = true;
                        fileUpload->fileUploadErrorDetails = "Remote cluster took too long to respond.";
                    }
                }

                // Check if the remote cluster reported an error about the upload
                // (e.g., invalid path, permission denied, disk full)
                if (fileUpload->fileUploadError)
                {
                    sendError(SimpleWeb::StatusCode::client_error_bad_request, fileUpload->fileUploadErrorDetails);
                    return;
                }

                // ============================================================
                // Step 4: Stream file data in chunks with backpressure management
                // ============================================================

                // Stream data with source-side pause/resume (backpressure algorithm)
                // We read from the HTTP stream and push to the WebSocket message queue.
                // If the queue gets too large, we pause reading to prevent memory exhaustion.
                const size_t CHUNK_SIZE = FILE_CHUNK_SIZE;
                std::vector<uint8_t> buffer(CHUNK_SIZE);
                uint64_t totalRead = 0;

                while (totalRead < fileSize)
                {
                    // Backpressure: Wait if WebSocket queue is getting too large
                    // This implements hysteresis to prevent thrashing:
                    //   - Pause if queue exceeds MAX_FILE_BUFFER_SIZE bytes
                    //   - Resume when queue drops below MIN_FILE_BUFFER_SIZE bytes
                    // This intentionally blocks the HTTP thread (and thus the client)
                    // rather than buffering unlimited data in memory.
                    if (!fileUpload->waitForQueueDrain(false))
                    {
                        throw std::runtime_error("Timeout waiting for queue to drain during upload");
                    }

                    // Check for errors from remote cluster (best-effort early abort)
                    // The remote might detect errors (e.g., disk full) mid-transfer
                    // We check periodically but don't synchronize perfectly - a few extra
                    // chunks may be sent after error, which is fine as the remote will drop them.
                    if (fileUpload->fileUploadError)
                    {
                        sendError(SimpleWeb::StatusCode::client_error_bad_request, fileUpload->fileUploadErrorDetails);
                        return;
                    }

                    // Read chunk from HTTP request stream into buffer
                    auto chunkSize = std::min(CHUNK_SIZE, static_cast<size_t>(fileSize - totalRead));
                    // Safe reinterpret_cast: char and uint8_t are both 8-bit types with identical
                    // representation, alignment (1), and no padding. The C++ standard guarantees
                    // char* can alias any type. This cast has zero runtime overhead - it's a
                    // compile-time type assertion only. istream::read() requires char* by spec,
                    // so this cast is unavoidable for binary I/O. Alternative approaches (memcpy
                    // through intermediate buffer) would add unnecessary memory copies and hurt
                    // performance. The lint exception is justified because this is idiomatic C++
                    // for binary stream I/O and poses no safety or portability concerns.
                    // NOLINTNEXTLINE(cppcoreguidelines-pro-type-reinterpret-cast)
                    request->content.read(reinterpret_cast<char*>(buffer.data()),
                                          static_cast<std::streamsize>(chunkSize));

                    // Verify we read the expected amount of data
                    if (request->content.gcount() != static_cast<std::streamsize>(chunkSize))
                    {
                        throw std::runtime_error("Unexpected end of file data");
                    }

                    // Send chunk to remote cluster via WebSocket
                    // Using Lowest priority to allow control messages (errors, completion) to jump ahead
                    // std::span provides zero-copy view into buffer - avoids copying chunk data again
                    auto chunkMsg = Message(FILE_UPLOAD_CHUNK, Message::Priority::Lowest, uuid);
                    chunkMsg.push_bytes(std::span<const uint8_t>(buffer.data(), chunkSize));
                    fileUpload->sendMessage(chunkMsg);

                    totalRead += chunkSize;
                }

                // ============================================================
                // Step 5: Wait for all chunks to be sent, then send completion
                // ============================================================

                // Wait for the message queue to be completely empty before sending completion message
                // This is critical because FILE_UPLOAD_COMPLETE has Highest priority, so if we
                // send it while FILE_UPLOAD_CHUNK messages (Lowest priority) are still queued,
                // it will jump ahead in the queue and arrive at the remote before all chunks.
                // This would cause the remote to complete the file prematurely with missing data.
                if (!fileUpload->waitForQueueDrain(true))
                {
                    throw std::runtime_error("Timeout waiting for queue to empty before sending completion");
                }

                // Final best-effort error check before sending completion
                // Ensure remote didn't report errors while we were draining the queue
                if (fileUpload->fileUploadError)
                {
                    sendError(SimpleWeb::StatusCode::client_error_bad_request, fileUpload->fileUploadErrorDetails);
                    return;
                }

                // Send completion message to remote cluster
                // Highest priority ensures this is processed immediately after all chunks
                auto completeMsg = Message(FILE_UPLOAD_COMPLETE, Message::Priority::Highest, uuid);
                fileUpload->sendMessage(completeMsg);

                // ============================================================
                // Step 6: Wait for remote cluster to confirm successful write
                // ============================================================

                // Wait for the remote cluster to respond with FILE_UPLOAD_COMPLETE confirmation
                // The remote will:
                //   1. Receive all chunks and write them to disk
                //   2. Flush/sync the file to ensure durability
                //   3. Send FILE_UPLOAD_COMPLETE back to us
                // If the remote encounters errors (e.g., disk full during write),
                // it will send FILE_UPLOAD_ERROR instead.
                {
                    std::unique_lock<std::mutex> lock(fileUpload->fileUploadDataCVMutex);

                    // Wait for completion confirmation or error from remote
                    if (!fileUpload->fileUploadDataCV.wait_for(lock,
                                                               std::chrono::seconds(CLIENT_TIMEOUT_SECONDS),
                                                               [&fileUpload] {
                                                                   return fileUpload->fileUploadComplete ||
                                                                          fileUpload->fileUploadError;
                                                               }))
                    {
                        throw std::runtime_error("Upload completion confirmation timeout");
                    }
                }

                // Check if the remote reported an error during final write/flush
                if (fileUpload->fileUploadError)
                {
                    sendError(SimpleWeb::StatusCode::client_error_bad_request, fileUpload->fileUploadErrorDetails);
                    return;
                }

                // ============================================================
                // Step 7: Return success to HTTP client
                // ============================================================

                // Upload completed successfully - file is now on remote cluster's disk
                nlohmann::json result;
                result.emplace("uploadId", uuid);
                result.emplace("status", "completed");

                SimpleWeb::CaseInsensitiveMultimap headers;
                headers.emplace("Content-Type", "application/json");

                response->write(SimpleWeb::StatusCode::success_ok, result.dump(), headers);
            }
            catch (std::exception& e)
            {
                dumpExceptions(e);
                sendError(SimpleWeb::StatusCode::client_error_bad_request, std::string("Bad request: ") + e.what());
            }
        };

    // List files in the specified directory
    server->getServer().resource["^" + path + "$"]["PATCH"] =
        [clusterManager, server, app](const std::shared_ptr<HttpServerImpl::Response>& response,
                                      const std::shared_ptr<HttpServerImpl::Request>& request) {
            // Verify that the user is authorized
            std::unique_ptr<sAuthorizationResult> authResult;
            try
            {
                authResult = server->isAuthorized(request->header);
            }
            catch (std::exception& e)
            {
                dumpExceptions(e);

                // Invalid request
                response->write(SimpleWeb::StatusCode::client_error_forbidden, "Not authorized");
                return;
            }

            try
            {
                // Create a vector which includes the application from the secret, and any other applications it has
                // access to
                auto applications = std::vector<std::string>({authResult->secret().name()});
                std::copy(authResult->secret().applications().begin(),
                          authResult->secret().applications().end(),
                          std::back_inserter(applications));

                // Read the json from the post body
                nlohmann::json post_data;
                request->content >> post_data;

                // Get the job to fetch files for if one was provided
                // NOLINTBEGIN(cppcoreguidelines-pro-bounds-avoid-unchecked-container-access)
                auto jobId = post_data.contains("jobId") ? static_cast<uint64_t>(post_data["jobId"]) : 0;

                // Get the job to fetch files for
                auto bRecursive = static_cast<bool>(post_data["recursive"]);

                // Get the path to the file to fetch (relative to the project)
                auto filePath = std::string{post_data["path"]};
                // NOLINTEND(cppcoreguidelines-pro-bounds-avoid-unchecked-container-access)

                if (jobId != 0)
                {
                    // Handle the file list request
                    handleFileList(app,
                                   clusterManager,
                                   jobId,
                                   bRecursive,
                                   filePath,
                                   authResult->secret().name(),
                                   applications,
                                   response);
                }
                else
                {
                    // A job ID was not provided, we need to use the cluster and bundle details from the json request
                    // Check that the bundle and cluster were provided in the POST json
                    if (!post_data.contains("cluster") || !post_data.contains("bundle"))
                    {
                        throw std::runtime_error(
                            "The 'cluster' and 'bundle' parameters were not provided in the absence of 'jobId'");
                    }

                    // NOLINTBEGIN(cppcoreguidelines-pro-bounds-avoid-unchecked-container-access)
                    auto sCluster = std::string{post_data["cluster"]};
                    auto sBundle  = std::string{post_data["bundle"]};
                    // NOLINTEND(cppcoreguidelines-pro-bounds-avoid-unchecked-container-access)

                    // Get the cluster to submit to
                    auto cluster = clusterManager->getCluster(sCluster);
                    if (!cluster)
                    {
                        // Invalid cluster
                        throw std::runtime_error("Invalid cluster");
                    }

                    // Confirm that the current JWT secret can access the provided cluster
                    const auto& clusters = authResult->secret().clusters();
                    if (std::ranges::find(clusters, sCluster) == clusters.end())
                    {
                        throw std::runtime_error("Application " + authResult->secret().name() +
                                                 " does not have access to cluster " + sCluster);
                    }

                    // Handle the file list request
                    handleFileList(app, cluster, sBundle, 0, bRecursive, filePath, response);
                }
            }
            catch (std::exception& exception)
            {
                dumpExceptions(exception);

                response->write(SimpleWeb::StatusCode::client_error_bad_request, "Bad request");
            }
        };
}
