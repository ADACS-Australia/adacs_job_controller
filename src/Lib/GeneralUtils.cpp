#include "GeneralUtils.h"
#include "../folly/folly/experimental/exception_tracer/ExceptionTracer.h"
#include <algorithm>
#include <boost/archive/iterators/base64_from_binary.hpp>
#include <boost/archive/iterators/binary_from_base64.hpp>
#include <boost/archive/iterators/remove_whitespace.hpp>
#include <boost/archive/iterators/transform_width.hpp>
#include <boost/lexical_cast.hpp>
#include <boost/uuid/uuid_generators.hpp>
#include <boost/uuid/uuid_io.hpp>
#include <iostream>
#include <execinfo.h>

// From https://github.com/kenba/via-httplib/blob/master/include/via/http/authentication/base64.hpp
auto base64Encode(std::string input) -> std::string
{
    // The input must be in multiples of 3, otherwise the transformation
    // may overflow the input buffer, so pad with zero.
    uint32_t num_pad_chars((3 - input.size() % 3) % 3);
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
        uint32_t num_pad_chars((4 - input.size() % 4) % 4);
        input.append(num_pad_chars, '=');

        uint32_t pad_chars(std::count(input.begin(), input.end(), '='));
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
    return boost::lexical_cast<std::string>(boost::uuids::random_generator()());
}

void dumpExceptions(std::exception& exception) {
    std::cerr << "--- Exception: " << exception.what() << std::endl;
    auto exceptions = folly::exception_tracer::getCurrentExceptions();
    for (auto& exc : exceptions) {
        std::cerr << exc << "\n";
    }
}

void handleSegv()
{
    void *array[10];
    int size;

    // get void*'s for all entries on the stack
    size = backtrace(array, 10);

    // print out all the frames to stderr
    fprintf(stderr, "Error: SEGFAULT:\n");
    backtrace_symbols_fd(array, size, STDERR_FILENO);

    throw std::runtime_error("Seg Fault Error");
}