//
// Created by lewis on 2/10/20.
//

#ifndef GWCLOUD_JOB_SERVER_GENERALUTILS_H
#define GWCLOUD_JOB_SERVER_GENERALUTILS_H

#include <chrono>
#include <condition_variable>
#include <mutex>
#include <string>

auto base64Encode(std::string input) -> std::string;
auto base64Decode(std::string input) -> std::string;
auto generateUUID() -> std::string;
void dumpExceptions(std::exception& exception);
void handleSegv();
auto acceptingConnections(uint16_t port) -> bool;

struct InterruptableTimer {
    // Returns false if killed
    template<class R, class P>
    auto wait_for( std::chrono::duration<R,P> const& time ) const -> bool {
        std::unique_lock<std::mutex> lock(m);
        return !cv.wait_for(lock, time, [&]{ return terminate; });
    }

    void stop() {
        std::unique_lock<std::mutex> const lock(m);
        terminate = true;
        cv.notify_all();
    }

private:
    mutable std::condition_variable cv;
    mutable std::mutex m;
    bool terminate = false;
};

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

#endif //GWCLOUD_JOB_SERVER_GENERALUTILS_H
