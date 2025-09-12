#include "GeneralUtils.h"
#include "../folly/folly/experimental/exception_tracer/ExceptionTracer.h"
#include "segvcatch.h"
#include <algorithm>
#include <boost/archive/iterators/base64_from_binary.hpp>
#include <boost/archive/iterators/binary_from_base64.hpp>
#include <boost/archive/iterators/remove_whitespace.hpp>
#include <boost/archive/iterators/transform_width.hpp>
#include <boost/asio.hpp>
#include <boost/lexical_cast.hpp>
#include <boost/system/error_code.hpp>
#include <boost/uuid/uuid_generators.hpp>
#include <boost/uuid/uuid_io.hpp>
#include <chrono>
#include <cstdint>
#include <cstdlib>
#include <exception>
#include <execinfo.h>
#include <folly/experimental/exception_tracer/ExceptionTracer.h>
#include <folly/experimental/exception_tracer/StackTrace.h>
#include <iostream>
#include <string>
#include <thread>

// Forward declaration for acceptingConnections function
auto acceptingConnections(uint16_t port) -> bool;

// From https://github.com/kenba/via-httplib/blob/master/include/via/http/authentication/base64.hpp
auto base64Encode(std::string input) -> std::string
{
    // The input must be in multiples of 3, otherwise the transformation
    // may overflow the input buffer, so pad with zero.
    const uint32_t num_pad_chars((3 - input.size() % 3) % 3);
    input.append(num_pad_chars, 0);

    // Transform to Base64
    using boost::archive::iterators::transform_width, boost::archive::iterators::base64_from_binary;
    // NOLINTNEXTLINE (cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
    using ItBase64T = base64_from_binary<transform_width<std::string::const_iterator, 6, 8>>;
    std::string output(ItBase64T(input.begin()),
                       ItBase64T(input.end() - num_pad_chars));

    // Pad blank characters with =
    output.append(num_pad_chars, '=');

    return output;
}

// From https://github.com/kenba/via-httplib/blob/master/include/via/http/authentication/base64.hpp
auto base64Decode(std::string input) -> std::string
{
    using boost::archive::iterators::transform_width, boost::archive::iterators::remove_whitespace, boost::archive::iterators::binary_from_base64;

    // NOLINTNEXTLINE (cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
    using ItBinaryT = transform_width<binary_from_base64<remove_whitespace<std::string::const_iterator>>, 8, 6>;

    try
    {
        // If the input isn't a multiple of 4, pad with =
        const uint32_t num_pad_chars((4 - input.size() % 4) % 4);
        input.append(num_pad_chars, '=');

        const uint32_t pad_chars(std::count(input.begin(), input.end(), '='));
        std::replace(input.begin(), input.end(), '=', 'A');
        std::string output(ItBinaryT(input.begin()), ItBinaryT(input.end()));
        output.erase(output.end() - pad_chars, output.end());
        return output;
    }
    catch (std::exception& e) 
    {
        dumpExceptions(e);
        return {""};
    }
}

auto generateUUID() -> std::string {
    auto uuid = boost::uuids::random_generator()();
    return boost::uuids::to_string(uuid);
}

void dumpExceptions(std::exception& exception) {
    std::cerr << "--- Exception: " << exception.what() << '\n';
    auto exceptions = folly::exception_tracer::getCurrentExceptions();
    for (auto& exc : exceptions) {
        std::cerr << exc << "\n";
    }
}

void handleSegv()
{
    // NOLINTBEGIN
    void *array[10];
    int size;

    // get void*'s for all entries on the stack
    size = backtrace(array, 10);

    // print out all the frames to stderr
    fprintf(stderr, "Error: SEGFAULT:\n");
    backtrace_symbols_fd(array, size, STDERR_FILENO);

    throw std::runtime_error("Seg Fault Error");
    // NOLINTEND
}

// NOLINTBEGIN(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
auto acceptingConnections(uint16_t port) -> bool {
    using boost::asio::io_service, boost::asio::deadline_timer, boost::asio::ip::tcp;
    using ec = boost::system::error_code;

    bool result = false;

    for (auto counter = 0; counter < 10 && !result; counter++) {
        try {
            io_service svc;
            tcp::socket socket(svc);
            boost::asio::steady_timer tim(svc, std::chrono::milliseconds(100));

            tim.async_wait([&](ec) { socket.cancel(); });
            socket.async_connect({{}, port}, [&](ec errorCode) {
                result = !errorCode;
            });

            svc.run();
        } catch(...) { 
            // Ignore connection errors during port checking
        }

        if (!result) {
            std::this_thread::sleep_for(std::chrono::milliseconds(100));
        }
    }

    return result;
}
// NOLINTEND(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)

// To prevent the compiler optimizing away the exception tracing from folly, we need to reference it.
extern "C" auto getCaughtExceptionStackTraceStack() -> const folly::exception_tracer::StackTrace*;
extern "C" auto getUncaughtExceptionStackTraceStack() -> const folly::exception_tracer::StackTraceStack*;

// forceExceptionStackTraceRef is intentionally unused and marked volatile so the compiler doesn't optimize away the
// required functions from folly. This is black magic.
volatile void forceExceptionStackTraceRef()
{
    getCaughtExceptionStackTraceStack();
    getUncaughtExceptionStackTraceStack();
}

// This is also black magic. What we're doing here is using this function as the initializer for the volatile static
// bool bForceStartup, which can't be optimized away. This function is guaranteed to be run during program startup
auto forceStartup() -> bool {
    // Set up the crash handler
    segvcatch::init_segv(&handleSegv);
    return true;
}

// NOLINTNEXTLINE(cppcoreguidelines-avoid-non-const-global-variables,cert-err58-cpp)
volatile bool bForceStartup = forceStartup();