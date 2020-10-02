#include <boost/archive/iterators/base64_from_binary.hpp>
#include <boost/archive/iterators/binary_from_base64.hpp>
#include <boost/archive/iterators/transform_width.hpp>
#include <boost/archive/iterators/remove_whitespace.hpp>
#include <algorithm>
#include <boost/lexical_cast.hpp>
#include <boost/uuid/uuid_generators.hpp>
#include <boost/uuid/uuid_io.hpp>

// From https://github.com/kenba/via-httplib/blob/master/include/via/http/authentication/base64.hpp
std::string base64Encode(std::string input)
{
    // The input must be in multiples of 3, otherwise the transformation
    // may overflow the input buffer, so pad with zero.
    size_t num_pad_chars((3 - input.size() % 3) % 3);
    input.append(num_pad_chars, 0);

    // Transform to Base64
    using namespace boost::archive::iterators;
    typedef base64_from_binary<transform_width
            <std::string::const_iterator, 6, 8> > ItBase64T;
    std::string output(ItBase64T(input.begin()),
                       ItBase64T(input.end() - num_pad_chars));

    // Pad blank characters with =
    output.append(num_pad_chars, '=');

    return output;
}

// From https://github.com/kenba/via-httplib/blob/master/include/via/http/authentication/base64.hpp
std::string base64Decode(std::string input)
{
    using namespace boost::archive::iterators;
    typedef transform_width<binary_from_base64<remove_whitespace
            <std::string::const_iterator> >, 8, 6> ItBinaryT;

    try
    {
        // If the input isn't a multiple of 4, pad with =
        size_t num_pad_chars((4 - input.size() % 4) % 4);
        input.append(num_pad_chars, '=');

        size_t pad_chars(std::count(input.begin(), input.end(), '='));
        std::replace(input.begin(), input.end(), '=', 'A');
        std::string output(ItBinaryT(input.begin()), ItBinaryT(input.end()));
        output.erase(output.end() - pad_chars, output.end());
        return output;
    }
    catch (std::exception const&)
    {
        return std::string("");
    }
}

std::string generateUUID() {
    return boost::lexical_cast<std::string>(boost::uuids::random_generator()());
}