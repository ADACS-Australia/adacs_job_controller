//
// Date Library C++20 Module Compatibility Shim
// Extracted from HTTP/File.cpp to fix date library template specialization issues
//

#pragma once

#include <chrono>
#include <iomanip>
#include <ostream>

#include <date/date.h>
#include <date/tz.h>

// We need to provide a specialization for the specific type that sqlpp11 uses
// The date library doesn't provide this specific template instantiation
namespace date::detail {

// NOLINTBEGIN(google-runtime-int,cert-dcl16-c,hicpp-uppercase-literal-suffix,readability-uppercase-literal-suffix,cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)

// Helper function to format hh_mm_ss with microseconds
template <class CharT, class Traits>
inline void format_hh_mm_ss_microseconds(std::basic_ostream<CharT, Traits>& os,
                                         const hh_mm_ss<std::chrono::duration<long, std::ratio<1L, 1000000L>>>& tod)
{
    auto f    = os.flags();
    auto fill = os.fill();

    os << std::setw(2) << std::setfill(CharT('0')) << tod.hours().count() << CharT(':') << std::setw(2)
       << tod.minutes().count() << CharT(':') << std::setw(2) << tod.seconds().count()
       << CharT('.')
       // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
       << std::setw(6) << std::setfill(CharT('0')) << tod.subseconds().count();

    os.fill(fill);
    os.flags(f);
}

}  // namespace date::detail

// Provide the operator<< in the global namespace to avoid conflicts
template <class CharT, class Traits>
inline std::basic_ostream<CharT, Traits>& operator<<(
    std::basic_ostream<CharT, Traits>& os,
    const date::hh_mm_ss<std::chrono::duration<long, std::ratio<1L, 1000000L>>>& tod)
{
    date::detail::format_hh_mm_ss_microseconds(os, tod);
    return os;
}

// NOLINTEND(google-runtime-int,cert-dcl16-c,hicpp-uppercase-literal-suffix,readability-uppercase-literal-suffix,cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)