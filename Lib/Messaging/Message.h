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

#define SYSTEM_SOURCE "system"

#define SERVER_READY 1000

class Cluster;

class Message {
public:
    enum Priority {
        Lowest = 19,
        Medium = 10,
        Highest = 0
    };

    Message(uint32_t msgId, Priority priority, const std::string& source);
    explicit Message(std::vector<unsigned char> vdata);

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
    void push_string(std::string v);
    void push_bytes(std::vector<uint8_t> v);

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

    uint32_t getId() {return id; }

private:
    std::vector<uint8_t> data;
    uint64_t index;
    Priority priority;
    std::string source;
    uint32_t id;
};


#endif //GWCLOUD_JOB_SERVER_MESSAGE_H
