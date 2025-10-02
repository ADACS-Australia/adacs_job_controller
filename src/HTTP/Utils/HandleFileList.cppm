//
// Created by lewis on 6/7/21.
//

module;
#include <chrono>
#include <filesystem>
#include <thread>

#include <boost/filesystem.hpp>
#include <boost/lexical_cast.hpp>
#include <boost/uuid/random_generator.hpp>
#include <boost/uuid/uuid_io.hpp>
#include <nlohmann/json.hpp>
#include <sqlpp11/sqlpp11.h>
#include <status_code.hpp>
#include <utility.hpp>

#include "../../Lib/FileTypes.h"
#include "../../Lib/FollyTypes.h"
#include "../../Lib/shims/date_shim.h"
#include "../../Lib/shims/sqlpp_shim.h"

export module HandleFileList;

import IApplication;
import IClusterManager;
import HttpServer;
import settings;
import MySqlConnector;
import jobserver_schema;
import Message;
import ICluster;
import GeneralUtils;

// Re-export sFile from FileTypes.h
export using sFile = ::sFile;

export void handleFileList(const std::shared_ptr<IApplication>& app,
                           const std::shared_ptr<IClusterManager>& clusterManager,
                           uint64_t jobId,
                           bool bRecursive,
                           const std::string& filePath,
                           const std::string& appName,
                           const std::vector<std::string>& applications,
                           const std::shared_ptr<HttpServerImpl::Response>& response);

export void handleFileList(const std::shared_ptr<IApplication>& app,
                           const std::shared_ptr<ICluster>& cluster,
                           const std::string& sBundle,
                           uint64_t jobId,
                           bool bRecursive,
                           const std::string& filePath,
                           const std::shared_ptr<HttpServerImpl::Response>& response);

export auto filterFiles(const std::vector<sFile>& files,
                        const std::string& filePath,
                        bool bRecursive) -> std::vector<sFile>;

auto filterFiles(const std::vector<sFile>& files, const std::string& filePath, bool bRecursive) -> std::vector<sFile>
{
    std::vector<sFile> matchedFiles;

    // Get the absolute path (Resolves paths like "/test/../test2/test/../myfile")
    auto absoluteFilePath = std::filesystem::weakly_canonical(std::filesystem::path(filePath)).string();

    // Make sure that the path has a leading and trailing slash so we correctly match only directories
    if (!absoluteFilePath.empty() && absoluteFilePath.back() != '/')
    {
        absoluteFilePath += '/';
    }

    if (absoluteFilePath.empty() || absoluteFilePath.front() != '/')
    {
        absoluteFilePath = "/" + absoluteFilePath;
    }

    // Create a version of the path with no trailing slash to match the base directory
    auto absoluteFilePathNoTrailingSlash =
        boost::filesystem::path(absoluteFilePath).remove_trailing_separator().string();

    for (const auto& file : files)
    {
        auto path = boost::filesystem::path(file.fileName);

        if (bRecursive)
        {
            // Make sure the parent path has a trailing slash
            auto parentPath = path.parent_path().string();
            if (!parentPath.empty() && parentPath.back() != '/')
            {
                parentPath += '/';
            }

            // If the path starts with absoluteFilePath then the file matches. Any files falling under this path match.
            if (path.string() == absoluteFilePath || parentPath.starts_with(absoluteFilePath) ||
                (path.string() == absoluteFilePathNoTrailingSlash && file.isDirectory))
            {
                matchedFiles.push_back(file);
            }
        }
        else
        {
            // If the parent path exactly matches the absoluteFilePath then the file matches.
            if (path.parent_path().string() == absoluteFilePath ||
                path.parent_path().string() == absoluteFilePathNoTrailingSlash ||
                (path.string() == absoluteFilePathNoTrailingSlash && file.isDirectory))
            {
                matchedFiles.push_back(file);
            }
        }
    }

    return matchedFiles;
}

// NOLINTNEXTLINE(misc-no-recursion)
void handleFileList(const std::shared_ptr<IApplication>& app,
                    const std::shared_ptr<ICluster>& cluster,
                    const std::string& sBundle,
                    uint64_t jobId,
                    bool bRecursive,
                    const std::string& filePath,
                    const std::shared_ptr<HttpServerImpl::Response>& response)
{
    // Create a database connection
    auto database = MySqlConnector();

    // Get the tables
    const schema::JobserverJob jobTable;
    const schema::JobserverJobhistory jobHistoryTable;
    const schema::JobserverFilelistcache fileListCacheTable;

    // Create a new file list object
    auto flObj = std::make_shared<sFileList>();

    // The job is not complete by default - to handle cases of file lists without a job ID
    bool jobComplete = false;

    // Generate a UUID for the message source
    auto uuid = boost::lexical_cast<std::string>(boost::uuids::random_generator()());

    try
    {
        // If the cluster isn't online then there isn't anything to do
        if (!cluster->isOnline())
        {
            if (response)
            {
                response->write(SimpleWeb::StatusCode::server_error_service_unavailable, "Remote Cluster Offline");
            }
            return;
        }

        if (jobId != 0)
        {
            // Check if this job is completed
            auto jobCompletionResult = database->run(
                select(all_of(jobHistoryTable))
                    .from(jobHistoryTable)
                    .where(jobHistoryTable.jobId == jobId and jobHistoryTable.what == "_job_completion_"));

            // If there is any record with "_job_completion_" for the specified job id, then the job is complete
            jobComplete = !jobCompletionResult.empty();

            if (jobComplete)
            {
                // Get any file list cache records for this job
                auto fileListCacheResult = database->run(select(all_of(fileListCacheTable))
                                                             .from(fileListCacheTable)
                                                             .where(fileListCacheTable.jobId == jobId));

                // If there are no file list cache records, then just fall through and let the websocket communication
                // write the records if required
                if (!fileListCacheResult.empty())
                {
                    std::vector<sFile> files;
                    for (const auto& cacheResult : fileListCacheResult)
                    {
                        files.push_back({.fileName    = cacheResult.path,
                                         .fileSize    = static_cast<uint64_t>(cacheResult.fileSize),
                                         .permissions = static_cast<uint32_t>(cacheResult.permissions),
                                         .isDirectory = static_cast<bool>(cacheResult.isDir)});
                    }

                    // Filter the files
                    auto filteredFiles = filterFiles(files, filePath, bRecursive);

                    // Iterate over the files and generate the result
                    nlohmann::json result;
                    // NOLINTBEGIN(cppcoreguidelines-pro-bounds-avoid-unchecked-container-access)
                    result["files"] = nlohmann::json::array();
                    for (const auto& file : filteredFiles)
                    {
                        nlohmann::json jFile;
                        jFile["path"]        = file.fileName;
                        jFile["isDir"]       = file.isDirectory;
                        jFile["fileSize"]    = file.fileSize;
                        jFile["permissions"] = file.permissions;

                        result["files"].push_back(jFile);
                    }
                    // NOLINTEND(cppcoreguidelines-pro-bounds-avoid-unchecked-container-access)

                    // Return the result
                    SimpleWeb::CaseInsensitiveMultimap headers;
                    headers.emplace("Content-Type", "application/json");

                    if (response)
                    {
                        response->write(SimpleWeb::StatusCode::success_ok, result.dump(), headers);
                    }
                    return;
                }
            }
        }

        // Store the file list object in the application map
        auto fileListMap = app->getFileListMap();
        fileListMap->emplace(uuid, flObj);

        // Create a message to request the file list
        auto msg = Message(FILE_LIST, Message::Priority::Highest, uuid);
        msg.push_uint(jobId);
        msg.push_string(uuid);
        msg.push_string(sBundle);
        msg.push_string(filePath);
        msg.push_bool(bRecursive);

        // Send the message to the cluster
        cluster->sendMessage(msg);

        {
            // Wait for the server to send back data, or in 30 seconds fail
            std::unique_lock<std::mutex> lock(flObj->dataCVMutex);

            // Wait for data to be ready to send
            if (!flObj->dataCV.wait_for(lock, std::chrono::seconds(CLIENT_TIMEOUT_SECONDS), [&flObj] {
                    return flObj->dataReady;
                }))
            {
                // Timeout reached, set the error
                flObj->error        = true;
                flObj->errorDetails = "Client too took long to respond.";
            }
        }

        // Check if the server received an error about the file list
        if (flObj->error)
        {
            {
                // Make sure we lock before removing an entry from the file list map in case Cluster() is using it
                const std::unique_lock<std::mutex> fileListMapDeletionLock(app->getFileListMapDeletionLockMutex());

                // Remove the file list object
                if (fileListMap->find(uuid) != fileListMap->end())
                {
                    fileListMap->erase(uuid);
                }
            }

            if (response)
            {
                response->write(SimpleWeb::StatusCode::client_error_bad_request, flObj->errorDetails);
            }
            return;
        }

        auto insert_query = insert_into(fileListCacheTable)
                                .columns(fileListCacheTable.jobId,
                                         fileListCacheTable.path,
                                         fileListCacheTable.isDir,
                                         fileListCacheTable.fileSize,
                                         fileListCacheTable.permissions,
                                         fileListCacheTable.timestamp);

        // Iterate over the files and generate the result
        nlohmann::json result;
        // NOLINTBEGIN(cppcoreguidelines-pro-bounds-avoid-unchecked-container-access)
        result["files"] = nlohmann::json::array();
        for (const auto& file : flObj->files)
        {
            nlohmann::json jFile;
            jFile["path"]        = file.fileName;
            jFile["isDir"]       = file.isDirectory;
            jFile["fileSize"]    = file.fileSize;
            jFile["permissions"] = file.permissions;

            result["files"].push_back(jFile);
            // NOLINTEND(cppcoreguidelines-pro-bounds-avoid-unchecked-container-access)

            // Check if the job is complete
            if (jobComplete)
            {
                // Insert the file list cache record
                insert_query.values.add(
                    fileListCacheTable.jobId       = static_cast<int>(jobId),
                    fileListCacheTable.path        = std::string(file.fileName),
                    fileListCacheTable.isDir       = file.isDirectory ? 1 : 0,
                    fileListCacheTable.fileSize    = static_cast<int64_t>(file.fileSize),
                    fileListCacheTable.permissions = static_cast<int>(file.permissions),
                    fileListCacheTable.timestamp =
                        std::chrono::time_point_cast<std::chrono::microseconds>(std::chrono::system_clock::now()));
            }
        }

        // Only save these records if the job is complete and the file path is "" (Root of the job), and the file list
        // was recursive. This criteria means that all jobs files will be returned to be cached
        if (jobComplete && filePath.empty() && bRecursive)
        {
            // Try inserting the values in the database
            try
            {
                // Start a transaction so we can catch an insertion error if required. This can happen if this
                // function is called more than once at the same time. (Two users might hit the same job in the UI
                // or API at the same time)
                database->start_transaction();

                // Insert the records  
                const auto insertCount = database->run(insert_query);
                // Count how many files should have been cached (only complete jobs)
                size_t expectedInserts = 0;
                for (const auto& file : flObj->files)
                {
                    if (jobComplete)
                        expectedInserts++;
                }
                
                if (insertCount != expectedInserts)
                {
                    std::cout << "Warning: File list cache insert mismatch for job " << jobId 
                              << ". Expected " << expectedInserts << " rows, got " << insertCount << '\n';
                }

                // Commit the changes in the database
                database->commit_transaction();
            }
            catch (sqlpp::exception& e)
            {
                dumpExceptions(e);

                // Uh oh, an error occurred
                // Abort the transaction
                database->rollback_transaction(false);
            }
        }
        else if (jobComplete)
        {
            // Somehow the job is complete, but there were no previous records - but the user has requested a
            // non job root directory and/or they haven't requested the file list to be recursive. Since we want
            // to reduce the number of times we hit the remote filesystem for the file list, let's call it once
            // more right now with the correct parameters to cache all files

            // Launch the file list in a new thread to prevent unexpected HTTP request delays. This is fine to run in
            // the background since it's a system operation at this point and not related to the original HTTP request.
            // We pass parameters by copy here intentionally, not by reference.
            std::thread([cluster, sBundle, jobId, &app] {
                handleFileList(app, cluster, sBundle, jobId, true, "", nullptr);
            }).detach();
        }

        {
            // Make sure we lock before removing an entry from the file list map in case Cluster() is using it
            const std::unique_lock<std::mutex> fileListMapDeletionLock(app->getFileListMapDeletionLockMutex());

            // Remove the file list object
            if (fileListMap->find(uuid) != fileListMap->end())
            {
                fileListMap->erase(uuid);
            }
        }

        // Return the result
        SimpleWeb::CaseInsensitiveMultimap headers;
        headers.emplace("Content-Type", "application/json");

        if (response)
        {
            response->write(SimpleWeb::StatusCode::success_ok, result.dump(), headers);
        }
    }
    catch (std::exception& exception)
    {
        dumpExceptions(exception);

        {
            // Make sure we lock before removing an entry from the file list map in case Cluster() is using it
            const std::unique_lock<std::mutex> fileListMapDeletionLock(app->getFileListMapDeletionLockMutex());

            // Remove the file list object
            auto fileListMap = app->getFileListMap();
            if (fileListMap->find(uuid) != fileListMap->end())
            {
                fileListMap->erase(uuid);
            }
        }

        // Report bad request
        if (response)
        {
            response->write(SimpleWeb::StatusCode::client_error_bad_request, "Bad request");
        }
    }
}

void handleFileList(
    // NOLINTNEXTLINE(bugprone-easily-swappable-parameters)
    const std::shared_ptr<IApplication>& app,
    const std::shared_ptr<IClusterManager>& clusterManager,
    uint64_t jobId,
    bool bRecursive,
    const std::string& filePath,
    const std::string& appName,
    const std::vector<std::string>& applications,
    const std::shared_ptr<HttpServerImpl::Response>& response)
{
    // Create a database connection
    auto database = MySqlConnector();

    // Get the tables
    const schema::JobserverJob jobTable;
    const schema::JobserverJobhistory jobHistoryTable;

    try
    {
        // Look up the job. If applications is empty, we ignore the application check. This is used internally for
        // caching files once the final job status is posted to the job server from the client. There is no way that
        // this function can be called from the HTTP side without having a valid non-empty applications list, since
        // it's configured in the JWT secrets config.
        auto jobResults = applications.empty()
                              ? database->run(select(all_of(jobTable)).from(jobTable).where(jobTable.id == jobId))
                              : database->run(select(all_of(jobTable))
                                                  .from(jobTable)
                                                  .where(jobTable.id == jobId and
                                                         jobTable.application.in(sqlpp::value_list(applications))));

        // Check that a job was actually found
        if (jobResults.empty())
        {
            throw std::runtime_error("Unable to find job with ID " + std::to_string(jobId) + " for application " +
                                     appName);
        }

        // Get the cluster and bundle from the job
        const auto* job = &jobResults.front();
        auto sCluster   = std::string{job->cluster};
        auto sBundle    = std::string{job->bundle};

        // Check that the cluster is online
        // Get the cluster to submit to
        auto cluster = clusterManager->getCluster(sCluster);
        if (!cluster)
        {
            // Invalid cluster
            throw std::runtime_error("Invalid cluster");
        }

        handleFileList(app, cluster, sBundle, job->id, bRecursive, filePath, response);
    }
    catch (std::exception& e)
    {
        dumpExceptions(e);

        response->write(SimpleWeb::StatusCode::client_error_bad_request, "Bad request");
    }
}
