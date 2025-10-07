//
// Created by lewis on 12/19/24.
//

module;

#include <condition_variable>
#include <iostream>
#include <memory>
#include <mutex>
#include <string>

export module FileUpload;

import settings;
import Cluster;
import Message;
import ICluster;
import IApplication;
import sClusterJob;

/**
 * FileUpload manages the WebSocket connection for a file upload session.
 *
 * This class represents the "sink" side of the upload: it receives file chunks from
 * the HTTP endpoint (source) and forwards them to the remote cluster via WebSocket.
 * It also handles backpressure by tracking message queue size and provides coordination
 * between the HTTP thread (pushing data) and WebSocket thread (consuming data).
 *
 * Lifetime: Created when HTTP PUT starts, destroyed when WebSocket disconnects.
 */
export class FileUpload : public Cluster
{
public:
    FileUpload(const std::shared_ptr<sClusterDetails>& details, std::string uuid, std::shared_ptr<IApplication> app);

    auto getUuid() -> std::string
    {
        return uuid;
    }

    void handleMessage(Message& message) override;
    void handleFileUploadError(Message& message);
    void handleFileUploadComplete(Message& message);

    // Upload state management - shared between HTTP thread (writer) and WebSocket thread (reader)
    uint64_t fileUploadFileSize = 0;      // Total file size in bytes
    bool fileUploadError        = false;  // Set by WebSocket thread if remote reports error
    std::string fileUploadErrorDetails;   // Error message from remote cluster
    bool fileUploadComplete     = false;  // Set when remote confirms upload complete
    bool fileUploadReceivedData = false;  // Set when any data or completion received

    // Synchronization: HTTP thread waits for WebSocket readiness/errors
    // The HTTP thread waits on this CV for the WebSocket connection to be ready,
    // or for errors/completion from the remote cluster. This prevents the HTTP
    // thread from sending data before the WebSocket is connected.
    mutable std::mutex fileUploadDataCVMutex;
    bool fileUploadDataReady = false;  // Signaled when: connection ready, error, or complete
    std::condition_variable fileUploadDataCV;

private:
    std::string uuid;  // Unique identifier for this upload session
};

FileUpload::FileUpload(const std::shared_ptr<sClusterDetails>& details,
                       std::string uuid,
                       std::shared_ptr<IApplication> app)
    : Cluster(details, std::move(app)), uuid(std::move(uuid))
{
    // Set role identifiers for logging and connection management
    role       = eRole::fileUpload;
    roleString = "file upload " + this->uuid;
}

void FileUpload::handleFileUploadError(Message& message)
{
    // Remote cluster reported an error (e.g., invalid path, permission denied, disk full)
    // Extract error details and signal the waiting HTTP thread to abort
    fileUploadErrorDetails = message.pop_string();
    fileUploadError        = true;

    // Wake up HTTP thread waiting for data readiness - it will check error flag and abort
    fileUploadDataReady = true;
    fileUploadDataCV.notify_one();
}

void FileUpload::handleFileUploadComplete(Message& /*message*/)
{
    // Remote cluster confirmed it received all chunks and wrote the file successfully
    // This is sent AFTER the remote has flushed all data to disk
    fileUploadComplete     = true;
    fileUploadReceivedData = true;

    // Signal HTTP thread that upload is complete - it can now return success to client
    fileUploadDataReady = true;
    fileUploadDataCV.notify_one();
}

void FileUpload::handleMessage(Message& message)
{
    auto msgId = message.getId();

    switch (msgId)
    {
        case SERVER_READY:
            // WebSocket connection is now established and authenticated
            // Signal HTTP thread that it can start sending file chunks
            fileUploadDataReady = true;
            fileUploadDataCV.notify_one();
            break;
        case FILE_UPLOAD_ERROR:
            // Remote cluster encountered an error - propagate to HTTP thread
            handleFileUploadError(message);
            break;
        case FILE_UPLOAD_COMPLETE:
            // Remote cluster successfully wrote the file - signal HTTP thread to return success
            handleFileUploadComplete(message);
            break;
        default:
            // Unexpected message type - log but don't crash (best-effort error handling)
            std::cout << "Got invalid message ID " << msgId << " from " << this->getName() << '\n';
    }
}