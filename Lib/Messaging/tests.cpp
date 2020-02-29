//
// Created by lewis on 2/26/20.
//
#include "Message.h"
#define BOOST_TEST_DYN_LINK
#define BOOST_TEST_MODULE MessageTest
#include <boost/test/unit_test.hpp>
#include <random>
#include <chrono>

BOOST_AUTO_TEST_CASE( test_primitive_ubyte )
{
    auto msg = Message(Message::Priority::Highest);

    msg.push_ubyte(1);
    msg.push_ubyte(5);
    msg.push_ubyte(245);

    BOOST_CHECK_EQUAL(msg.pop_ubyte(), 1);
    BOOST_CHECK_EQUAL(msg.pop_ubyte(), 5);
    BOOST_CHECK_EQUAL(msg.pop_ubyte(), 245);
}

BOOST_AUTO_TEST_CASE( test_primitive_byte )
{
    auto msg = Message(Message::Priority::Highest);

    msg.push_byte(1);
    msg.push_byte(-120);
    msg.push_byte(120);

    BOOST_CHECK_EQUAL(msg.pop_byte(), 1);
    BOOST_CHECK_EQUAL(msg.pop_byte(), -120);
    BOOST_CHECK_EQUAL(msg.pop_byte(), 120);
}

BOOST_AUTO_TEST_CASE( test_primitive_ushort )
{
    auto msg = Message(Message::Priority::Highest);

    msg.push_ushort(0x1);
    msg.push_ushort(0x1200);
    msg.push_ushort(0xffff);

    BOOST_CHECK_EQUAL(msg.pop_ushort(), 0x1);
    BOOST_CHECK_EQUAL(msg.pop_ushort(), 0x1200);
    BOOST_CHECK_EQUAL(msg.pop_ushort(), 0xffff);
}

BOOST_AUTO_TEST_CASE( test_primitive_short )
{
    auto msg = Message(Message::Priority::Highest);

    msg.push_short(0x1);
    msg.push_short(-0x1200);
    msg.push_short(0x2020);

    BOOST_CHECK_EQUAL(msg.pop_short(), 0x1);
    BOOST_CHECK_EQUAL(msg.pop_short(), -0x1200);
    BOOST_CHECK_EQUAL(msg.pop_short(), 0x2020);
}

BOOST_AUTO_TEST_CASE( test_primitive_uint )
{
    auto msg = Message(Message::Priority::Highest);

    msg.push_uint(0x1);
    msg.push_uint(0x12345678);
    msg.push_uint(0xffff1234);

    BOOST_CHECK_EQUAL(msg.pop_uint(), 0x1);
    BOOST_CHECK_EQUAL(msg.pop_uint(), 0x12345678);
    BOOST_CHECK_EQUAL(msg.pop_uint(), 0xffff1234);
}

BOOST_AUTO_TEST_CASE( test_primitive_int )
{
    auto msg = Message(Message::Priority::Highest);

    msg.push_int(0x1);
    msg.push_int(-0x12345678);
    msg.push_int(0x12345678);

    BOOST_CHECK_EQUAL(msg.pop_int(), 0x1);
    BOOST_CHECK_EQUAL(msg.pop_int(), -0x12345678);
    BOOST_CHECK_EQUAL(msg.pop_int(), 0x12345678);
}

BOOST_AUTO_TEST_CASE( test_primitive_ulong )
{
    auto msg = Message(Message::Priority::Highest);

    msg.push_ulong(0x1);
    msg.push_ulong(0x1234567812345678);
    msg.push_ulong(0xffff123412345678);

    BOOST_CHECK_EQUAL(msg.pop_ulong(), 0x1);
    BOOST_CHECK_EQUAL(msg.pop_ulong(), 0x1234567812345678);
    BOOST_CHECK_EQUAL(msg.pop_ulong(), 0xffff123412345678);
}

BOOST_AUTO_TEST_CASE( test_primitive_long )
{
    auto msg = Message(Message::Priority::Highest);

    msg.push_long(0x1);
    msg.push_long(-0x1234567812345678);
    msg.push_long(0x1234567812345678);

    BOOST_CHECK_EQUAL(msg.pop_long(), 0x1);
    BOOST_CHECK_EQUAL(msg.pop_long(), -0x1234567812345678);
    BOOST_CHECK_EQUAL(msg.pop_long(), 0x1234567812345678);
}

BOOST_AUTO_TEST_CASE( test_primitive_float, * boost::unit_test::tolerance(0.00001) )
{
    auto msg = Message(Message::Priority::Highest);

    msg.push_float(0.1f);
    msg.push_float(0.1234567812345678f);
    msg.push_float(-0.1234123f);

    BOOST_CHECK_EQUAL(msg.pop_float(), 0.1f);
    BOOST_CHECK_EQUAL(msg.pop_float(), 0.1234567812345678f);
    BOOST_CHECK_EQUAL(msg.pop_float(), -0.1234123f);
}

BOOST_AUTO_TEST_CASE( test_primitive_double, * boost::unit_test::tolerance(0.00001) )
{
    auto msg = Message(Message::Priority::Highest);

    msg.push_double(0.1);
    msg.push_double(0.1234567812345678);
    msg.push_double(-0.1234567812345678);

    BOOST_CHECK_EQUAL(msg.pop_double(), 0.1);
    BOOST_CHECK_EQUAL(msg.pop_double(), 0.1234567812345678);
    BOOST_CHECK_EQUAL(msg.pop_double(), -0.1234567812345678);
}

BOOST_AUTO_TEST_CASE( test_primitive_string )
{
    auto msg = Message(Message::Priority::Highest);

    auto s1 = std::string("Hello!");
    auto s2 = std::string("Hello again!");
    auto s3 = std::string("Hello one last time!");

    msg.push_string(s1);
    msg.push_string(s2);
    msg.push_string(s3);

    BOOST_CHECK_EQUAL(msg.pop_string(), s1);
    BOOST_CHECK_EQUAL(msg.pop_string(), s2);
    BOOST_CHECK_EQUAL(msg.pop_string(), s3);
}

BOOST_AUTO_TEST_CASE( test_primitive_bytes ) {
    std::random_device engine;

    std::vector<uint8_t> data;
    for (auto i = 0; i < 512; i++)
        data.push_back(engine());

    auto msg = Message(Message::Priority::Highest);
    msg.push_bytes(data);

    auto result = msg.pop_bytes();

    BOOST_CHECK_EQUAL_COLLECTIONS(data.begin(), data.end(), result.begin(), result.end());
}

std::chrono::milliseconds get_millis() {
    return duration_cast<std::chrono::milliseconds >(
            std::chrono::system_clock::now().time_since_epoch()
    );
}

BOOST_AUTO_TEST_CASE( test_primitive_bytes_rate )
{
    std::random_device engine;

    // First try 512b chunks
    auto time_now = get_millis();
    auto time_next = time_now + std::chrono::seconds(5);

    std::vector<uint8_t> data;
    for (auto i = 0; i < 512; i++)
        data.push_back(engine());

    {
        auto msg = Message(Message::Priority::Highest);
        msg.push_bytes(data);

        auto result = msg.pop_bytes();

        BOOST_CHECK_EQUAL_COLLECTIONS(data.begin(), data.end(), result.begin(), result.end());
    }

    uint64_t counter = 0;
    while (get_millis() < time_next)
    {
        auto msg = Message(Message::Priority::Highest);
        msg.push_bytes(data);

        auto result = msg.pop_bytes();
        counter += result.size();
    }

    std::cout << "Message raw bytes throughput 512b chunks Mb/s: " << counter/1024.f/1024.f/5 << std::endl;

    // First try 1Mb chunks
    time_now = get_millis();
    time_next = time_now + std::chrono::seconds(5);

    data.clear();
    for (auto i = 0; i < 1024*1024; i++)
        data.push_back(engine());

    counter = 0;
    while (get_millis() < time_next)
    {
        auto msg = Message(Message::Priority::Highest);
        msg.push_bytes(data);

        auto result = msg.pop_bytes();
        counter += result.size();
    }

    std::cout << "Message raw bytes throughput 1Mb chunks Mb/s: " << counter/1024.f/1024.f/5 << std::endl;
}