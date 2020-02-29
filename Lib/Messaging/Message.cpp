//
// Created by lewis on 2/26/20.
//

#include "Message.h"

#include <utility>
#include "../../Cluster/Cluster.h"

using namespace std;

Message::Message(Message::Priority priority) {
    // Resize the data array to 64kb
    data.reserve(1024 * 64);

    // Reset the index
    index = 0;

    // Set the priority
    this->priority = priority;
}

Message::Message(vector<uint8_t> vdata) {
    data = std::move(vdata);
    index = 0;
}

void Message::push_ubyte(uint8_t v) {
    data.push_back(v);
}

uint8_t Message::pop_ubyte() {
    auto result = data[index++];
    return result;
}

void Message::push_byte(int8_t v) {
    push_ubyte((uint8_t) v);
}

int8_t Message::pop_byte() {
    return (int8_t) pop_ubyte();
}

#define push_type(t, r) void Message::push_##t (r v) {      \
    uint8_t pdata[sizeof(v)];                       \
                                                    \
    *((typeof(v)*) &pdata) = v;                     \
                                                    \
    for (unsigned char i : pdata)                   \
        push_ubyte(i);                                \
}

#define pop_type(t, r) r Message::pop_##t() {       \
    uint8_t pdata[sizeof(r)];               \
                                                    \
    for (auto i = 0; i < sizeof(r); i++)    \
        pdata[i] = pop_ubyte();                     \
                                                    \
    return *(r*) &pdata;                          \
}

#define add_type(t, r) push_type(t, r) pop_type(t, r)

add_type(ushort, uint16_t)

add_type(short, int16_t)

add_type(uint, uint32_t)

add_type(int, int32_t)

add_type(ulong, uint64_t)

add_type(long, int64_t)

add_type(float, float)

add_type(double, double)

void Message::push_string(std::string v) {
    push_ulong(v.size());
    data.insert(data.end(), v.begin(), v.end());
}

std::string Message::pop_string() {
    auto result = pop_bytes();
    // Write string terminator
    result.push_back(0);
    return std::string((char *) result.data());
}

void Message::push_bytes(std::vector<uint8_t> v) {
    push_ulong(v.size());
    data.insert(data.end(), v.begin(), v.end());
}

std::vector<uint8_t> Message::pop_bytes() {
    auto len = pop_ulong();
    auto result = std::vector<uint8_t>(&data[index], &data[index] + len);
    index += len;
    return result;
}

void Message::send(Cluster* pCluster) {
    pCluster->queueMessage(&data, priority);
}


