//
// Created by lewis on 6/7/21.
//
#include "HandleFileList.h"
#include <boost/lexical_cast.hpp>
#include <boost/uuid/uuid_io.hpp>
#include <boost/uuid/random_generator.hpp>
#include "boost/filesystem.hpp"
#include "../../DB/MySqlConnector.h"
#include "../../Lib/jobserver_schema.h"
#include <filesystem>

using namespace schema;

std::vector<sFile> filterFiles(const std::vector<sFile> &files, const std::string &filePath, bool bRecursive) {
    std::vector<sFile> matchedFiles;

    // Get the absolute path (Resolves paths like "/test/../test2/test/../myfile")
    auto absoluteFilePath = std::filesystem::weakly_canonical(std::filesystem::path(filePath)).string();

    // Make sure that the path has a leading and trailing slash so we correctly match only directories
    if (!absoluteFilePath.empty() && absoluteFilePath.back() != '/')
        absoluteFilePath += '/';

    if (absoluteFilePath.empty() || absoluteFilePath.front() != '/')
        absoluteFilePath = "/" + absoluteFilePath;

    // Create a version of the path with no trailing slash to match the base directory
    auto absoluteFilePathNoTrailingSlash = boost::filesystem::path(absoluteFilePath).remove_trailing_separator().string();

    for (const auto &f : files) {
        auto p = boost::filesystem::path(f.fileName);

        if (bRecursive) {
            // Make sure the parent path has a trailing slash
            auto parentPath = p.parent_path().string();
            if (!parentPath.empty() && parentPath.back() != '/')
                parentPath += '/';

            // If the path starts with absoluteFilePath then the file matches. Any files falling under this path match.
            if (
                    p.string() == absoluteFilePath
                    || parentPath.rfind(absoluteFilePath, 0) == 0
                    || (p.string() == absoluteFilePathNoTrailingSlash && f.isDirectory)
                    ) {
                matchedFiles.push_back(f);
            }
        } else {
            // If the parent path exactly matches the absoluteFilePath then the file matches.
            if (
                    p.parent_path().string() == absoluteFilePath
                    || p.parent_path().string() == absoluteFilePathNoTrailingSlash
                    || (p.string() == absoluteFilePathNoTrailingSlash && f.isDirectory)
                    ) {
                matchedFiles.push_back(f);
            }
        }
    }

    return matchedFiles;
}

void handleFileList(
        std::shared_ptr<Cluster> cluster, auto job, bool bRecursive, const std::string &filePath,
        const std::string &appName, const std::vector<std::string> &applications,
        HttpServerImpl::Response *response
) {
    // Create a database connection
    auto db = MySqlConnector();

    // Get the tables
    JobserverJob jobTable;
    JobserverJobhistory jobHistoryTable;
    JobserverFilelistcache fileListCacheTable;

    // Create a new file list object
    auto flObj = std::make_shared<sFileList>();

    // Generate a UUID for the message source
    auto uuid = boost::lexical_cast<std::string>(boost::uuids::random_generator()());

    try {
        // If the cluster isn't online then there isn't anything to do
        if (!cluster->isOnline()) {
            if (response)
                response->write(SimpleWeb::StatusCode::server_error_service_unavailable, "Remote Cluster Offline");
            return;
        }

        // Check if this job is completed
        auto jobCompletionResult = db->run(
                select(all_of(jobHistoryTable))
                        .from(jobHistoryTable)
                        .where(
                                jobHistoryTable.jobId == (uint32_t) job->id
                                and jobHistoryTable.what == "_job_completion_"
                        )
        );

        // If there is any record with "_job_completion_" for the specified job id, then the job is complete
        auto jobComplete = !jobCompletionResult.empty();

        if (jobComplete) {
            // Get any file list cache records for this job
            auto fileListCacheResult = db->run(
                    select(all_of(fileListCacheTable))
                            .from(fileListCacheTable)
                            .where(
                                    fileListCacheTable.jobId == (uint32_t) job->id
                            )
            );

            // If there are no file list cache records, then just fall through and let the websocket communication
            // write the records if required
            if (!fileListCacheResult.empty()) {

                std::vector<sFile> files;
                for (auto &f : fileListCacheResult) {
                    files.push_back({
                                            f.path,
                                            (uint64_t) f.fileSize,
                                            (uint32_t) f.permissions,
                                            (bool) f.isDir
                                    });
                }

                // Filter the files
                auto filteredFiles = filterFiles(files, filePath, bRecursive);

                // Iterate over the files and generate the result
                nlohmann::json result;
                result["files"] = nlohmann::json::array();
                for (const auto &f : filteredFiles) {
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

                if (response) response->write(SimpleWeb::StatusCode::success_ok, result.dump(), headers);
                return;
            }
        }

        // Cluster is online, create the file list object
        fileListMap->emplace(uuid, flObj);

        // Send a message to the client to initiate the file list
        auto msg = Message(FILE_LIST, Message::Priority::Highest, uuid);
        msg.push_uint((uint32_t) job->id);
        msg.push_string(uuid);
        msg.push_string(std::string(job->bundle));
        msg.push_string(filePath);
        msg.push_bool(bRecursive);
        msg.send(cluster);

        {
            // Wait for the server to send back data, or in 30 seconds fail
            std::unique_lock<std::mutex> lock(flObj->dataCVMutex);

            // Wait for data to be ready to send
            if (!flObj->dataCV.wait_for(lock, std::chrono::seconds(30), [&flObj] { return flObj->dataReady; })) {
                // Timeout reached, set the error
                flObj->error = true;
                flObj->errorDetails = "Client too took long to respond.";
            }
        }

        // Check if the server received an error about the file list
        if (flObj->error) {
            {
                // Make sure we lock before removing an entry from the file list map in case Cluster() is using it
                std::unique_lock<std::mutex> fileListMapDeletionLock(fileListMapDeletionLockMutex);

                // Remove the file list object
                if (fileListMap->find(uuid) != fileListMap->end())
                    fileListMap->erase(uuid);
            }

            if (response) response->write(SimpleWeb::StatusCode::client_error_bad_request, flObj->errorDetails);
            return;
        }

        auto insert_query = insert_into(fileListCacheTable).columns(
                fileListCacheTable.jobId,
                fileListCacheTable.path,
                fileListCacheTable.isDir,
                fileListCacheTable.fileSize,
                fileListCacheTable.permissions,
                fileListCacheTable.timestamp
        );

        // Iterate over the files and generate the result
        nlohmann::json result;
        result["files"] = nlohmann::json::array();
        for (const auto &f : flObj->files) {
            nlohmann::json jFile;
            jFile["path"] = f.fileName;
            jFile["isDir"] = f.isDirectory;
            jFile["fileSize"] = f.fileSize;
            jFile["permissions"] = f.permissions;

            result["files"].push_back(jFile);

            // Check if the job is complete
            if (jobComplete) {
                // Insert the file list cache record
                insert_query.values.add(
                        fileListCacheTable.jobId = (int32_t) job->id,
                        fileListCacheTable.path = std::string(f.fileName),
                        fileListCacheTable.isDir = f.isDirectory ? 1 : 0,
                        fileListCacheTable.fileSize = (long long) f.fileSize,
                        fileListCacheTable.permissions = (int) f.permissions,
                        fileListCacheTable.timestamp =
                                std::chrono::time_point_cast<std::chrono::microseconds>(
                                        std::chrono::system_clock::now()
                                )
                );
            }
        }

        // Only save these records if the job is complete and the file path is "" (Root of the job), and the file list
        // was recursive. This criteria means that all jobs files will be returned to be cached
        if (jobComplete && filePath.empty() && bRecursive) {
            // Try inserting the values in the database
            try {
                // Start a transaction so we can catch an insertion error if required. This can happen if this
                // function is called more than once at the same time. (Two users might hit the same job in the UI
                // or API at the same time)
                db->start_transaction();

                // Insert the records
                db->run(insert_query);

                // Commit the changes in the database
                db->commit_transaction();
            } catch (sqlpp::exception& e) {
                dumpExceptions(e);

                // Uh oh, an error occurred
                // Abort the transaction
                db->rollback_transaction(false);
            }
        } else if (jobComplete) {
            // Somehow the job is complete, but there were no previous records - but the user has requested a
            // non job root directory and/or they haven't requested the file list to be recursive. Since we want
            // to reduce the number of times we hit the remote filesystem for the file list, let's call it once
            // more right now with the correct parameters to cache all files

            ::handleFileList(cluster, job, true, "", "job_controller", {}, nullptr);
        }

        {
            // Make sure we lock before removing an entry from the file list map in case Cluster() is using it
            std::unique_lock<std::mutex> fileListMapDeletionLock(fileListMapDeletionLockMutex);

            // Remove the file list object
            if (fileListMap->find(uuid) != fileListMap->end())
                fileListMap->erase(uuid);
        }

        // Return the result
        SimpleWeb::CaseInsensitiveMultimap headers;
        headers.emplace("Content-Type", "application/json");

        if (response) response->write(SimpleWeb::StatusCode::success_ok, result.dump(), headers);
    } catch (std::exception& e) {
        dumpExceptions(e);

        {
            // Make sure we lock before removing an entry from the file list map in case Cluster() is using it
            std::unique_lock<std::mutex> fileListMapDeletionLock(fileListMapDeletionLockMutex);

            // Remove the file list object
            if (fileListMap->find(uuid) != fileListMap->end())
                fileListMap->erase(uuid);
        }

        // Report bad request
        if (response) response->write(SimpleWeb::StatusCode::client_error_bad_request, "Bad request");
    }
}

void handleFileList(
        std::shared_ptr<ClusterManager> clusterManager, uint32_t jobId, bool bRecursive, const std::string &filePath,
        const std::string &appName, const std::vector<std::string> &applications, HttpServerImpl::Response *response
) {
    // Create a database connection
    auto db = MySqlConnector();

    // Get the tables
    JobserverJob jobTable;
    JobserverJobhistory jobHistoryTable;
    JobserverFilelistcache fileListCacheTable;

    try {
        // Look up the job. If applications is empty, we ignore the application check. This is used internally for
        // caching files once the final job status is posted to the job server from the client. There is no way that
        // this function can be called from the HTTP side without having a valid non-empty applications list, since
        // it's configured in the JWT secrets config.
        auto jobResults =
                applications.empty() ?
                db->run(
                        select(all_of(jobTable))
                                .from(jobTable)
                                .where(
                                        jobTable.id == (uint32_t) jobId
                                )
                )
                                     :
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
            throw std::runtime_error(
                    "Unable to find job with ID " + std::to_string(jobId) + " for application " + appName);
        }

        // Get the cluster and bundle from the job
        auto job = &jobResults.front();
        auto sCluster = std::string(job->cluster);
        auto sBundle = std::string(job->bundle);

        // Check that the cluster is online
        // Get the cluster to submit to
        auto cluster = clusterManager->getCluster(sCluster);
        if (!cluster) {
            // Invalid cluster
            throw std::runtime_error("Invalid cluster");
        }

        handleFileList(cluster, job, bRecursive, filePath, appName, applications, response);
    } catch (std::exception& e) {
        dumpExceptions(e);

        response->write(SimpleWeb::StatusCode::client_error_bad_request, "Bad request");
    }
}