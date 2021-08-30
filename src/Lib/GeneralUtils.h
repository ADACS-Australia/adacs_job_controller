//
// Created by lewis on 2/10/20.
//

#ifndef GWCLOUD_JOB_SERVER_GENERALUTILS_H
#define GWCLOUD_JOB_SERVER_GENERALUTILS_H

#include <string>

std::string base64Encode(std::string input);
std::string base64Decode(std::string input);
std::string generateUUID();
void dumpExceptions(std::exception& e);

#ifdef BUILD_TESTS
#define EXPOSE_PROPERTY_FOR_TESTING(x) public: auto get##x () { return &x; } auto set##x (typeof(x) v) { x = v; }
#define EXPOSE_PROPERTY_FOR_TESTING_READONLY(x) public: auto get##x () { return &x; }
#define EXPOSE_FUNCTION_FOR_TESTING(x) public: auto call##x () { return x(); }
#define EXPOSE_FUNCTION_FOR_TESTING_ONE_PARAM(x, y) public: auto call##x (y p1) { return x(p1); }
#else
// Noop
#define EXPOSE_PROPERTY_FOR_TESTING(x)
#define EXPOSE_PROPERTY_FOR_TESTING_READONLY(x)
#define EXPOSE_FUNCTION_FOR_TESTING(x)
#define EXPOSE_FUNCTION_FOR_TESTING_ONE_PARAM(x, y)
#endif

#endif //GWCLOUD_JOB_SERVER_GENERALUTILS_H
