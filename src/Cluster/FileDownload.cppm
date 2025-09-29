//
// Created by lewis on 10/15/22.
//

module;
#include <condition_variable>
#include <iostream>
#include <memory>
#include <mutex>
#include <string>
#include <utility>

#include <folly/concurrency/UnboundedQueue.h>
export module FileDownload;

import settings;
import ICluster;
import Cluster;
import Message;
import IApplication;

export class FileDownload : public Cluster
{
public:
    FileDownload(const std::shared_ptr<sClusterDetails>& details, std::string uuid, std::shared_ptr<IApplication> app);

    auto getUuid() -> std::string
    {
        return uuid;
    }

    void handleMessage(Message& message) override;
    void handleFileChunk(Message& message);
    void handleFileDetails(Message& message);
    void handleFileError(Message& message);

    folly::USPSCQueue<std::shared_ptr<std::vector<uint8_t>>, false> fileDownloadQueue;
    uint64_t fileDownloadFileSize = -1;
    bool fileDownloadError        = false;
    std::string fileDownloadErrorDetails;
    mutable std::mutex fileDownloadDataCVMutex;
    bool fileDownloadDataReady = false;
    std::condition_variable fileDownloadDataCV;
    bool fileDownloadReceivedData      = false;
    uint64_t fileDownloadReceivedBytes = 0;
    uint64_t fileDownloadSentBytes     = 0;
    bool fileDownloadClientPaused      = false;

private:
    std::string uuid;
};

FileDownload::FileDownload(const std::shared_ptr<sClusterDetails>& details,
                           std::string uuid,
                           std::shared_ptr<IApplication> app)
    : Cluster(details, std::move(app)), uuid(std::move(uuid))
{
    role       = eRole::fileDownload;
    roleString = "file download " + this->uuid;
}

void FileDownload::handleFileChunk(Message& message)
{
    auto chunk = message.pop_bytes();

    fileDownloadReceivedBytes += chunk.size();

    // Copy the chunk and push it on to the queue
    fileDownloadQueue.enqueue(std::make_shared<std::vector<uint8_t>>(chunk));

    {
        // The Pause/Resume messages must be synchronized to avoid a deadlock
        std::unique_lock<std::mutex> fileDownloadPauseResumeLock(app->getFileDownloadPauseResumeLockMutex());

        if (!fileDownloadClientPaused)
        {
            // Check if our buffer is too big
            if (fileDownloadReceivedBytes - fileDownloadSentBytes > MAX_FILE_BUFFER_SIZE)
            {
                // Ask the client to pause the file transfer
                fileDownloadClientPaused = true;

                auto msg = Message(PAUSE_FILE_CHUNK_STREAM, Message::Priority::Highest, uuid);
                sendMessage(msg);
            }
        }
    }

    // Trigger the file transfer event
    fileDownloadDataReady = true;
    fileDownloadDataCV.notify_one();
}

void FileDownload::handleFileDetails(Message& message)
{
    // Set the file size
    fileDownloadFileSize = message.pop_ulong();

    fileDownloadReceivedData = true;

    // Trigger the file transfer event
    fileDownloadDataReady = true;
    fileDownloadDataCV.notify_one();
}

void FileDownload::handleFileError(Message& message)
{
    fileDownloadError        = true;
    fileDownloadErrorDetails = message.pop_string();

    // Trigger the file transfer event
    fileDownloadDataReady = true;
    fileDownloadDataCV.notify_one();
}

void FileDownload::handleMessage(Message& message)
{
    switch (message.getId())
    {
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
            std::cout << "FileDownload: Unknown message type: " << message.getId() << std::endl;
            break;
    }
}
