//
// Created by lewis on 10/15/22.
//

import settings;

#include "FileDownload.h"
#include <utility>

FileDownload::FileDownload(const std::shared_ptr<sClusterDetails>& details, std::string uuid) : Cluster(details), uuid(std::move(uuid)) {
    role = eRole::fileDownload;
    roleString = "file download " + this->uuid;
}

void FileDownload::handleFileChunk(Message &message) {
    auto chunk = message.pop_bytes();

    fileDownloadReceivedBytes += chunk.size();

    // Copy the chunk and push it on to the queue
    fileDownloadQueue.enqueue(std::make_shared<std::vector<uint8_t>>(chunk));

    {
        // The Pause/Resume messages must be synchronized to avoid a deadlock
        std::unique_lock<std::mutex> fileDownloadPauseResumeLock(fileDownloadPauseResumeLockMutex);

        if (!fileDownloadClientPaused) {
            // Check if our buffer is too big
            if (fileDownloadReceivedBytes - fileDownloadSentBytes > MAX_FILE_BUFFER_SIZE) {
                // Ask the client to pause the file transfer
                fileDownloadClientPaused = true;

                auto msg = Message(PAUSE_FILE_CHUNK_STREAM, Message::Priority::Highest, uuid);
                msg.send(shared_from_this());
            }
        }
    }

    // Trigger the file transfer event
    fileDownloadDataReady = true;
    fileDownloadDataCV.notify_one();
}

void FileDownload::handleFileDetails(Message &message) {
    // Set the file size
    fileDownloadFileSize = message.pop_ulong();

    fileDownloadReceivedData = true;

    // Trigger the file transfer event
    fileDownloadDataReady = true;
    fileDownloadDataCV.notify_one();
}

void FileDownload::handleFileError(Message &message) {
    // Set the error
    fileDownloadErrorDetails = message.pop_string();
    fileDownloadError = true;

    // Trigger the file transfer event
    fileDownloadDataReady = true;
    fileDownloadDataCV.notify_one();
}

void FileDownload::handleMessage(Message &message) {
    auto msgId = message.getId();

    switch (msgId) {
        case FILE_CHUNK:
            handleFileChunk(message);
            break;
        case FILE_DETAILS:
            handleFileDetails(message);
            break;
        case FILE_ERROR:
            handleFileError(message);
            break;
        default:
            std::cout << "Got invalid message ID " << msgId << " from " << this->getName() << std::endl;
    }
}
