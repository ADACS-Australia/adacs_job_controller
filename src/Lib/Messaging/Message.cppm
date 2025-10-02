//
// Created by lewis on 2/26/20.
//

module;
#include <array>
#include <cstdint>
#include <cstring>
#include <memory>
#include <ostream>
#include <string>
#include <vector>

#include "../TestingMacros.h"

export module Message;

import settings;
import GeneralUtils;

// Message type constants
export constexpr const char* SYSTEM_SOURCE = "system";

export constexpr uint32_t SERVER_READY = 1000;

export constexpr uint32_t SUBMIT_JOB = 2000;
export constexpr uint32_t UPDATE_JOB = 2001;
export constexpr uint32_t CANCEL_JOB = 2002;
export constexpr uint32_t DELETE_JOB = 2003;

export constexpr uint32_t DOWNLOAD_FILE            = 4000;
export constexpr uint32_t FILE_DETAILS             = 4001;
export constexpr uint32_t FILE_ERROR               = 4002;
export constexpr uint32_t FILE_CHUNK               = 4003;
export constexpr uint32_t PAUSE_FILE_CHUNK_STREAM  = 4004;
export constexpr uint32_t RESUME_FILE_CHUNK_STREAM = 4005;
export constexpr uint32_t FILE_LIST                = 4006;
export constexpr uint32_t FILE_LIST_ERROR          = 4007;

// ClusterJob DB messages
export constexpr uint32_t DB_JOB_GET_BY_JOB_ID    = 5000;
export constexpr uint32_t DB_JOB_GET_BY_ID        = 5001;
export constexpr uint32_t DB_JOB_GET_RUNNING_JOBS = 5002;
export constexpr uint32_t DB_JOB_DELETE           = 5003;
export constexpr uint32_t DB_JOB_SAVE             = 5004;

// ClusterJobStatus DB messages
export constexpr uint32_t DB_JOBSTATUS_GET_BY_JOB_ID_AND_WHAT = 6000;
export constexpr uint32_t DB_JOBSTATUS_GET_BY_JOB_ID          = 6001;
export constexpr uint32_t DB_JOBSTATUS_DELETE_BY_ID_LIST      = 6002;
export constexpr uint32_t DB_JOBSTATUS_SAVE                   = 6003;

export constexpr uint32_t DB_RESPONSE = 7000;

export constexpr uint32_t DB_BUNDLE_CREATE_OR_UPDATE_JOB = 8000;
export constexpr uint32_t DB_BUNDLE_GET_JOB_BY_ID        = 8001;
export constexpr uint32_t DB_BUNDLE_DELETE_JOB           = 8002;

export class Message
{
public:
    enum class Priority : std::uint8_t
    {
        Lowest  = 19,
        Medium  = 10,
        Highest = 0
    };

    // Printable representation so Boost.Test can output Priority values
    friend std::ostream& operator<<(std::ostream& os, const Priority& p)
    {
        switch (p)
        {
            case Priority::Highest:
                os << "Highest";
                break;
            case Priority::Medium:
                os << "Medium";
                break;
            case Priority::Lowest:
                os << "Lowest";
                break;
        }
        return os;
    }

#ifdef BUILD_TESTS
    explicit Message(uint32_t msgId) : index(0), id(msgId)
    {
        // Constructor only used for testing
        data = std::make_shared<std::vector<uint8_t>>();
        data->reserve(MESSAGE_INITIAL_VECTOR_SIZE);
    }
#endif

    Message(uint32_t msgId, Priority priority, const std::string& source) : priority(priority), index(0), source(source)
    {
        data = std::make_shared<std::vector<uint8_t>>();
        data->reserve(MESSAGE_INITIAL_VECTOR_SIZE);

        // Push the source
        push_string(source);

        // Push the id
        push_uint(msgId);
    }

    explicit Message(const std::vector<uint8_t>& vdata)
        : data(std::make_shared<std::vector<uint8_t>>(vdata)), index(0), source(pop_string()), id(pop_uint())
    {}

    void push_bool(bool value)
    {
        push_ubyte(value ? 1 : 0);
    }

    [[nodiscard]] auto pop_bool() -> bool
    {
        auto result = pop_ubyte();
        return result == 1;
    }

    void push_ubyte(uint8_t value)
    {
        data->push_back(value);
    }

    [[nodiscard]] auto pop_ubyte() -> uint8_t
    {
        auto result = data->at(index++);
        return result;
    }

    void push_byte(int8_t value)
    {
        push_ubyte(static_cast<uint8_t>(value));
    }

    [[nodiscard]] auto pop_byte() -> int8_t
    {
        return static_cast<int8_t>(pop_ubyte());
    }

    void push_ushort(uint16_t value)
    {
        std::array<uint8_t, sizeof(value)> data_array{};
        memcpy(data_array.data(), &value, sizeof(value));
        for (const unsigned char nextbyte : data_array)
        {
            push_ubyte(nextbyte);
        }
    }

    [[nodiscard]] auto pop_ushort() -> uint16_t
    {
        std::array<uint8_t, sizeof(uint16_t)> data_array{};
        for (auto i = 0; i < sizeof(uint16_t); i++)
        {
            data_array.at(i) = pop_ubyte();
        }
        uint16_t result = 0;
        memcpy(&result, data_array.data(), sizeof(result));
        return result;
    }

    void push_short(int16_t value)
    {
        std::array<uint8_t, sizeof(value)> data_array{};
        memcpy(data_array.data(), &value, sizeof(value));
        for (const unsigned char nextbyte : data_array)
        {
            push_ubyte(nextbyte);
        }
    }

    [[nodiscard]] auto pop_short() -> int16_t
    {
        std::array<uint8_t, sizeof(int16_t)> data_array{};
        for (auto i = 0; i < sizeof(int16_t); i++)
        {
            data_array.at(i) = pop_ubyte();
        }
        int16_t result = 0;
        memcpy(&result, data_array.data(), sizeof(result));
        return result;
    }

    void push_uint(uint32_t value)
    {
        std::array<uint8_t, sizeof(value)> data_array{};
        memcpy(data_array.data(), &value, sizeof(value));
        for (const unsigned char nextbyte : data_array)
        {
            push_ubyte(nextbyte);
        }
    }

    [[nodiscard]] auto pop_uint() -> uint32_t
    {
        std::array<uint8_t, sizeof(uint32_t)> data_array{};
        for (auto i = 0; i < sizeof(uint32_t); i++)
        {
            data_array.at(i) = pop_ubyte();
        }
        uint32_t result = 0;
        memcpy(&result, data_array.data(), sizeof(result));
        return result;
    }

    void push_int(int32_t value)
    {
        std::array<uint8_t, sizeof(value)> data_array{};
        memcpy(data_array.data(), &value, sizeof(value));
        for (const unsigned char nextbyte : data_array)
        {
            push_ubyte(nextbyte);
        }
    }

    [[nodiscard]] auto pop_int() -> int32_t
    {
        std::array<uint8_t, sizeof(int32_t)> data_array{};
        for (auto i = 0; i < sizeof(int32_t); i++)
        {
            data_array.at(i) = pop_ubyte();
        }
        int32_t result = 0;
        memcpy(&result, data_array.data(), sizeof(result));
        return result;
    }

    void push_ulong(uint64_t value)
    {
        std::array<uint8_t, sizeof(value)> data_array{};
        memcpy(data_array.data(), &value, sizeof(value));
        for (const unsigned char nextbyte : data_array)
        {
            push_ubyte(nextbyte);
        }
    }

    [[nodiscard]] auto pop_ulong() -> uint64_t
    {
        std::array<uint8_t, sizeof(uint64_t)> data_array{};
        for (auto i = 0; i < sizeof(uint64_t); i++)
        {
            data_array.at(i) = pop_ubyte();
        }
        uint64_t result = 0;
        memcpy(&result, data_array.data(), sizeof(result));
        return result;
    }

    void push_long(int64_t value)
    {
        std::array<uint8_t, sizeof(value)> data_array{};
        memcpy(data_array.data(), &value, sizeof(value));
        for (const unsigned char nextbyte : data_array)
        {
            push_ubyte(nextbyte);
        }
    }

    [[nodiscard]] auto pop_long() -> int64_t
    {
        std::array<uint8_t, sizeof(int64_t)> data_array{};
        for (auto i = 0; i < sizeof(int64_t); i++)
        {
            data_array.at(i) = pop_ubyte();
        }
        int64_t result = 0;
        memcpy(&result, data_array.data(), sizeof(result));
        return result;
    }

    void push_float(float value)
    {
        std::array<uint8_t, sizeof(value)> data_array{};
        memcpy(data_array.data(), &value, sizeof(value));
        for (const unsigned char nextbyte : data_array)
        {
            push_ubyte(nextbyte);
        }
    }

    [[nodiscard]] auto pop_float() -> float
    {
        std::array<uint8_t, sizeof(float)> data_array{};
        for (auto i = 0; i < sizeof(float); i++)
        {
            data_array.at(i) = pop_ubyte();
        }
        float result = 0.0F;
        memcpy(&result, data_array.data(), sizeof(result));
        return result;
    }

    void push_double(double value)
    {
        std::array<uint8_t, sizeof(value)> data_array{};
        memcpy(data_array.data(), &value, sizeof(value));
        for (const unsigned char nextbyte : data_array)
        {
            push_ubyte(nextbyte);
        }
    }

    [[nodiscard]] auto pop_double() -> double
    {
        std::array<uint8_t, sizeof(double)> data_array{};
        for (auto i = 0; i < sizeof(double); i++)
        {
            data_array.at(i) = pop_ubyte();
        }
        double result = 0.0;
        memcpy(&result, data_array.data(), sizeof(result));
        return result;
    }

    void push_string(const std::string& value)
    {
        push_ulong(value.size());
        data->insert(data->end(), value.begin(), value.end());
    }

    [[nodiscard]] auto pop_string() -> std::string
    {
        auto result = pop_bytes();
        return {result.begin(), result.end()};
    }

    void push_bytes(const std::vector<uint8_t>& value)
    {
        push_ulong(value.size());
        data->insert(data->end(), value.begin(), value.end());
    }

    [[nodiscard]] auto pop_bytes() -> std::vector<uint8_t>
    {
        auto len     = pop_ulong();
        auto result  = std::vector<uint8_t>(data->begin() + static_cast<int64_t>(index),
                                           data->begin() + static_cast<int64_t>(index) + static_cast<int64_t>(len));
        index       += len;
        return result;
    }

    // Message no longer knows about clusters - clusters send messages

    [[nodiscard]] auto getId() const -> uint32_t
    {
        return id;
    }

    [[nodiscard]] auto getSource() const -> const std::string&
    {
        return source;
    }

    [[nodiscard]] auto getPriority() const -> Priority
    {
        return priority;
    }

    [[nodiscard]] auto getData() const -> const std::shared_ptr<std::vector<uint8_t>>&
    {
        return data;
    }

private:
    std::shared_ptr<std::vector<uint8_t>> data;
    uint64_t index;
    Priority priority = Priority::Lowest;
    std::string source;
    uint32_t id = 0;

    // For testing - we'll need to handle this specially
    friend class TestCluster;

#ifdef BUILD_TESTS
public:
    // Testing accessor for data member
    EXPOSE_PROPERTY_FOR_TESTING(data);
#endif
};
