//
// SQLPP11 C++20 Module Compatibility Shim
// Extracted from sClusterJobStatus.cpp to fix C++20 module problems
//

#pragma once

#include <string_view>

#include <sqlpp11/sqlpp11.h>

namespace sqlpp {
// 3-parameter form: (tuple, sep, context)
// template <typename Tuple, typename Context>
// inline Context& interpret_tuple(const Tuple& t, const char* sep, Context& ctx)
//{
//  const char c = (sep && *sep) ? *sep : '\0';
//  return interpret_tuple(t, c, ctx);
//}

// template <typename Tuple, typename Context>
// inline Context& interpret_tuple(const Tuple& t, std::string_view sep, Context& ctx)
//{
//   const char c = (!sep.empty()) ? sep.front() : '\0';
//   return interpret_tuple(t, c, ctx);
// }

// 4-parameter form seen in some headers: (tuple, sep, context, useBraces)
// template <typename Tuple, typename Context, typename UseBraces>
// inline Context& interpret_tuple(const Tuple& t, const char* sep, Context& ctx, UseBraces useBraces)
//{
//  const char c = (sep && *sep) ? *sep : '\0';
//  return interpret_tuple(t, c, ctx, useBraces);
//}

// template <typename Tuple, typename Context, typename UseBraces>
// inline Context& interpret_tuple(const Tuple& t, std::string_view sep, Context& ctx, UseBraces useBraces)
//{
//   const char c = (!sep.empty()) ? sep.front() : '\0';
//   return interpret_tuple(t, c, ctx, useBraces);
// }

// 5-parameter element bridge: (element, sep, context, useBraces, index)
// Prints the separator for index>0, then delegates to serialize().
// template <typename Element, typename Separator, typename Context, typename UseBraces>
// inline void interpret_tuple_element(const Element& e,
//                                    Separator sep,
//                                    Context& ctx,
//                                    UseBraces /*useBraces*/,
//                                    std::size_t index)
//{
//  if (index)
//    ctx << sep;
//
//  using ::sqlpp::serialize; // enable ADL
//  serialize(e, ctx);
//}

// 5-arg form when separator is a single char
template <typename Element, typename Context, typename UseBraces>
inline void interpret_tuple_element(const Element& e,
                                    char sep,
                                    Context& ctx,
                                    UseBraces /*useBraces*/,
                                    std::size_t index)
{
    if (index)
    {
        ctx << sep;
    }

    using ::sqlpp::serialize;  // enable ADL
    serialize(e, ctx);
}

// 5-arg form when separator is a string literal (e.g. ",")
template <typename Element, typename Context, typename UseBraces>
inline void interpret_tuple_element(const Element& e,
                                    const char* sep,
                                    Context& ctx,
                                    UseBraces useBraces,
                                    std::size_t index)
{
    // forward to the char version with the first character
    const char c = (sep && *sep) ? *sep : '\0';
    interpret_tuple_element(e, c, ctx, useBraces, index);
}

}  // namespace sqlpp
