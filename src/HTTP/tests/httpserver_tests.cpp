//
// Created by lewis on 2/10/20.
//

#include <boost/test/unit_test.hpp>
#include <jwt/jwt.hpp>
#include <server_http.hpp>

import settings;
import HttpServer;
import Application;
import GeneralUtils;
import IHttpServer;

// NOLINTBEGIN(concurrency-mt-unsafe)
BOOST_AUTO_TEST_SUITE(HttpServer_test_suite)
/*
 * This test suite is responsible for testing the HttpServer class
 */

    // Define several clusters and set the environment variable
    const auto sAccess = R"(
    [
        {
            "name": "app1",
            "secret": "super_secret1"
        },
        {
            "name": "app2",
            "secret": "super_secret2"
        },
        {
            "name": "app3",
            "secret": "super_secret3"
        }
    ]
    )";

    BOOST_AUTO_TEST_CASE(test_constructor) {
        /*
         * Test HttpServer constructor through Application
         */
        {
            // First check that instantiating Application with no access config works as expected
            unsetenv(ACCESS_SECRET_ENV_VARIABLE);
            auto app = createApplication();
            auto httpServer = app->getHttpServer();
            auto concreteHttpServer = std::static_pointer_cast<HttpServer>(httpServer);
            BOOST_CHECK_EQUAL(concreteHttpServer->getvJwtSecrets()->size(), 0);
        }

        setenv(ACCESS_SECRET_ENV_VARIABLE, base64Encode(sAccess).c_str(), 1);
        auto app = createApplication();
        auto httpServer = app->getHttpServer();
        auto concreteHttpServer = std::static_pointer_cast<HttpServer>(httpServer);

        // Double check that the secrets json was correctly parsed
        BOOST_CHECK_EQUAL(concreteHttpServer->getvJwtSecrets()->size(), 3);
        for (auto i = 1; i <= 3; i++) {
            BOOST_CHECK_EQUAL(concreteHttpServer->getvJwtSecrets()->at(i - 1).name(), "app" + std::to_string(i));
            BOOST_CHECK_EQUAL(concreteHttpServer->getvJwtSecrets()->at(i - 1).secret(), "super_secret" + std::to_string(i));
        }
    }

    // NOLINTNEXTLINE(readability-function-cognitive-complexity)
    BOOST_AUTO_TEST_CASE(test_isAuthorized) {
        /*
         * Test HttpServer->isAuthorized() function
         */
        {
            // Check that authorization without any config denies access without crashing
            unsetenv(ACCESS_SECRET_ENV_VARIABLE);
            auto app = createApplication();
            auto httpServer = app->getHttpServer();
            auto concreteHttpServer = std::static_pointer_cast<HttpServer>(httpServer);
            concreteHttpServer->getvJwtSecrets()->clear();

            auto headers = SimpleWeb::CaseInsensitiveMultimap();
            BOOST_CHECK_THROW(concreteHttpServer->isAuthorized(headers), eNotAuthorized);
        }

        setenv(ACCESS_SECRET_ENV_VARIABLE, base64Encode(sAccess).c_str(), 1);
        auto app = createApplication();
        auto httpServer = app->getHttpServer();
        auto concreteHttpServer = std::static_pointer_cast<HttpServer>(httpServer);

        jwt::jwt_object jwtToken {
                jwt::params::algorithm("HS256"),
                jwt::params::payload({
                    {"userId", "5"},
                    {"userName", "User"}
                }),
                jwt::params::secret("notarealsecret")
        };

        // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
        auto now = std::chrono::system_clock::now() + std::chrono::minutes{10};
        jwtToken.add_claim("exp", now);

        // Try using an invalid secret
        auto headers = SimpleWeb::CaseInsensitiveMultimap();
        headers.emplace("Authorization", jwtToken.signature());
        BOOST_CHECK_THROW(concreteHttpServer->isAuthorized(headers), eNotAuthorized);

        // Try using all 3 valid secrets
        for (auto& jwtSecret : *concreteHttpServer->getvJwtSecrets()) {
            jwtToken = {
                    jwt::params::algorithm("HS256"),
                    jwt::params::payload({
                                                 {"userId", "5"},
                                                 {"userName", "User"}
                                         }),
                    jwt::params::secret(jwtSecret.secret())
            };
            jwtToken.add_claim("exp", now);

            headers = SimpleWeb::CaseInsensitiveMultimap();
            headers.emplace("Authorization", jwtToken.signature());

            BOOST_CHECK_NO_THROW(concreteHttpServer->isAuthorized(headers));

            // Compare the json return value from isAuthorized for a valid secret
            BOOST_CHECK_EQUAL(concreteHttpServer->isAuthorized(headers)->payload(), nlohmann::json::object({
                {"exp", std::chrono::duration_cast<std::chrono::seconds>(now.time_since_epoch()).count()},
                {"userId", "5"},
                {"userName", "User"}
            }));

            BOOST_CHECK_EQUAL(concreteHttpServer->isAuthorized(headers)->secret().secret(), jwtSecret.secret());
        }

        // Try using one more invalid secret
        jwtToken = {
                jwt::params::algorithm("HS256"),
                jwt::params::payload({
                                             {"userId", "5"},
                                             {"userName", "User"}
                                     }),
                jwt::params::secret(concreteHttpServer->getvJwtSecrets()->at(0).secret() + "notreal")
        };
        jwtToken.add_claim("exp", now);

        headers = SimpleWeb::CaseInsensitiveMultimap();
        headers.emplace("Authorization", jwtToken.signature());
        BOOST_CHECK_THROW(concreteHttpServer->isAuthorized(headers), eNotAuthorized);
    }
BOOST_AUTO_TEST_SUITE_END()
// NOLINTEND(concurrency-mt-unsafe)