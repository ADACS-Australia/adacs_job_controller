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

export class FileUpload : public Cluster {
public:
    FileUpload(const std::shared_ptr<sClusterDetails>& details, std::string uuid, std::shared_ptr<IApplication> app);

    auto getUuid() -> std::string {
        return uuid;
    }

    void handleMessage(Message &message) override;
    void handleFileUploadError(Message &message);
    void handleFileUploadComplete(Message &message);

    // Upload state management
    uint64_t fileUploadFileSize = 0;
    bool fileUploadError = false;
    std::string fileUploadErrorDetails;
    bool fileUploadComplete = false;
    bool fileUploadReceivedData = false;

    // Buffer management for sink (WebSocket)
    mutable std::mutex fileUploadDataCVMutex;
    bool fileUploadDataReady = false;
    std::condition_variable fileUploadDataCV;

private:
    std::string uuid;
};

FileUpload::FileUpload(const std::shared_ptr<sClusterDetails>& details, std::string uuid, std::shared_ptr<IApplication> app) : Cluster(details, std::move(app)), uuid(std::move(uuid)) {
    role = eRole::fileUpload;
    roleString = "file upload " + this->uuid;
}

void FileUpload::handleFileUploadError(Message &message) {
    // Set the error
    fileUploadErrorDetails = message.pop_string();
    fileUploadError = true;

    // Trigger the file transfer event
    fileUploadDataReady = true;
    fileUploadDataCV.notify_one();
}

void FileUpload::handleFileUploadComplete(Message &/*message*/) {
    // Mark upload as complete
    fileUploadComplete = true;
    fileUploadReceivedData = true;

    // Trigger the file transfer event
    fileUploadDataReady = true;
    fileUploadDataCV.notify_one();
}

void FileUpload::handleMessage(Message &message) {
    auto msgId = message.getId();

    switch (msgId) {
        case SERVER_READY:
            // Connection is established and ready to receive file data
            fileUploadDataReady = true;
            fileUploadDataCV.notify_one();
            break;
        case FILE_UPLOAD_ERROR:
            handleFileUploadError(message);
            break;
        case FILE_UPLOAD_COMPLETE:
            handleFileUploadComplete(message);
            break;
        default:
            // Invalid message ID - ignore
            break;
    }
}