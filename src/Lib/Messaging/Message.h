//
// Created by lewis on 2/26/20.
//

#ifndef GWCLOUD_JOB_SERVER_MESSAGE_H
#define GWCLOUD_JOB_SERVER_MESSAGE_H

#include <vector>
#include <deque>
#include <cstdint>
#include <string>
#include "../../WebSocket/WebSocketServer.h"
#include "../GeneralUtils.h"

#define SYSTEM_SOURCE "system"

#define SERVER_READY 1000

#define SUBMIT_JOB 2000
#define UPDATE_JOB 2001

#define REQUEST_BUNDLE 3000

#define DOWNLOAD_FILE 4000
#define FILE_DETAILS 4001
#define FILE_ERROR 4002
#define FILE_CHUNK 4003
#define PAUSE_FILE_CHUNK_STREAM 4004
#define RESUME_FILE_CHUNK_STREAM 4005
#define FILE_LIST 4006
#define FILE_LIST_ERROR 4007

class Cluster;

class Message {
public:
    enum Priority {
        Lowest = 19,
        Medium = 10,
        Highest = 0
    };

#ifdef BUILD_TESTS
    Message(uint32_t msgId);
#endif

    Message(uint32_t msgId, Priority priority, const std::string& source);
    explicit Message(const std::vector<unsigned char>& vdata);

    void push_bool(bool v);
    void push_ubyte(uint8_t v);
    void push_byte(int8_t v);
    void push_ushort(uint16_t v);
    void push_short(int16_t v);
    void push_uint(uint32_t v);
    void push_int(int32_t v);
    void push_ulong(uint64_t v);
    void push_long(int64_t v);
    void push_float(float v);
    void push_double(double v);
    void push_string(const std::string& v);
    void push_bytes(const std::vector<uint8_t>& v);

    bool pop_bool();
    uint8_t pop_ubyte();
    int8_t pop_byte();
    uint16_t pop_ushort();
    int16_t pop_short();
    uint32_t pop_uint();
    int32_t pop_int();
    uint64_t pop_ulong();
    int64_t pop_long();
    float pop_float();
    double pop_double();
    std::string pop_string();
    std::vector<uint8_t> pop_bytes();

    void send(Cluster* pCluster);

    uint32_t getId() const { return id; }

private:
    std::vector<uint8_t> data;
    uint64_t index;
    Priority priority;
    std::string source;
    uint32_t id;

EXPOSE_PROPERTY_FOR_TESTING(data);
};


#endif //GWCLOUD_JOB_SERVER_MESSAGE_H
