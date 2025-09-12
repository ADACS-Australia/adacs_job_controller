//
// Created by lewis on 2/10/20.
//

#ifndef GWCLOUD_JOB_SERVER_TESTINGMACROS_H
#define GWCLOUD_JOB_SERVER_TESTINGMACROS_H

#ifdef BUILD_TESTS
// NOLINTBEGIN
#define EXPOSE_PROPERTY_FOR_TESTING(term) public: auto get##term () { return &term; } auto set##term (typeof(term) value) { term = value; }
#define EXPOSE_PROPERTY_FOR_TESTING_READONLY(term) public: auto get##term () { return &term; }
#define EXPOSE_FUNCTION_FOR_TESTING(term) public: auto call##term () { return term(); }
#define EXPOSE_FUNCTION_FOR_TESTING_ONE_PARAM(term, param) public: auto call##term (param value) { return term(value); }
// NOLINTEND
#else
// Noop
#define EXPOSE_PROPERTY_FOR_TESTING(x)
#define EXPOSE_PROPERTY_FOR_TESTING_READONLY(x)
#define EXPOSE_FUNCTION_FOR_TESTING(x)
#define EXPOSE_FUNCTION_FOR_TESTING_ONE_PARAM(x, y)
#endif

#endif //GWCLOUD_JOB_SERVER_TESTINGMACROS_H
