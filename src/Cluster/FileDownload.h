//
// Created by lewis on 10/15/22.
//

#ifndef GWCLOUD_JOB_SERVER_FILEDOWNLOAD_H
#define GWCLOUD_JOB_SERVER_FILEDOWNLOAD_H


#include "Cluster.h"

class FileDownload : public Cluster {
public:
    FileDownload(const std::shared_ptr<sClusterDetails>& details, std::string uuid);

    std::string getRoleString() override {
        return "file download " + uuid;
    }

    eRole getRole() override {
        return eRole::fileDownload;
    }

    auto getUuid() -> std::string {
        return uuid;
    }

    void handleMessage(Message &message) override;
    void handleFileChunk(Message &message);
    void handleFileDetails(Message &message);
    void handleFileError(Message &message);


    folly::USPSCQueue<std::shared_ptr<std::vector<uint8_t>>, false> fileDownloadQueue;
    uint64_t fileDownloadFileSize = -1;
    bool fileDownloadError = false;
    std::string fileDownloadErrorDetails;
    mutable std::mutex fileDownloadDataCVMutex;
    bool fileDownloadDataReady = false;
    std::condition_variable fileDownloadDataCV;
    bool fileDownloadReceivedData = false;
    uint64_t fileDownloadReceivedBytes = 0;
    uint64_t fileDownloadSentBytes = 0;
    bool fileDownloadClientPaused = false;

private:
    std::string uuid;
};


#endif //GWCLOUD_JOB_SERVER_FILEDOWNLOAD_H
