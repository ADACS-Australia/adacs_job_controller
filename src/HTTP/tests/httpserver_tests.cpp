//
// Created by lewis on 2/10/20.
//

#include <boost/test/unit_test.hpp>
#include "../HttpServer.h"
#include "../../Settings.h"
#include <jwt/jwt.hpp>

BOOST_AUTO_TEST_SUITE(HttpServer_test_suite)
/*
 * This test suite is responsible for testing the HttpServer class
 */

    // Define several clusters and set the environment variable
    auto sAccess = R"(
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
         * Test HttpServer constructor
         */

        // First check that instantiating HttpServer with no access config works as expected
        auto svr = new HttpServer(nullptr);
        BOOST_CHECK_EQUAL(svr->getvJwtSecrets()->size(), 0);

        setenv(ACCESS_SECRET_ENV_VARIABLE, base64Encode(sAccess).c_str(), 1);
        svr = new HttpServer(nullptr);

        // Double check that the secrets json was correctly parsed
        BOOST_CHECK_EQUAL(svr->getvJwtSecrets()->size(), 3);
        for (auto i = 1; i <= 3; i++) {
            BOOST_CHECK_EQUAL(svr->getvJwtSecrets()->at(i - 1).name(), "app" + std::to_string(i));
            BOOST_CHECK_EQUAL(svr->getvJwtSecrets()->at(i - 1).secret(), "super_secret" + std::to_string(i));
        }
    }

    BOOST_AUTO_TEST_CASE(test_isAuthorized) {
        /*
         * Test HttpServer->isAuthorized() function
         */

        // Check that authorization without any config denies access without crashing
        auto svr = new HttpServer(nullptr);
        svr->getvJwtSecrets()->clear();

        auto headers = SimpleWeb::CaseInsensitiveMultimap();
        BOOST_CHECK_THROW(svr->isAuthorized(headers), eNotAuthorized);

        setenv(ACCESS_SECRET_ENV_VARIABLE, base64Encode(sAccess).c_str(), 1);
        svr = new HttpServer(nullptr);

        jwt::jwt_object jwtToken {
                jwt::params::algorithm("HS256"),
                jwt::params::payload({
                    {"userId", "5"},
                    {"userName", "User"}
                }),
                jwt::params::secret("notarealsecret")
        };

        auto now = std::chrono::system_clock::now() + std::chrono::seconds{10};
        jwtToken.add_claim("exp", now);

        // Try using an invalid secret
        headers = SimpleWeb::CaseInsensitiveMultimap();
        headers.emplace("Authorization", jwtToken.signature());
        BOOST_CHECK_THROW(svr->isAuthorized(headers), eNotAuthorized);

        // Try using all 3 valid secrets
        for (auto& s : *svr->getvJwtSecrets()) {
            jwtToken = {
                    jwt::params::algorithm("HS256"),
                    jwt::params::payload({
                                                 {"userId", "5"},
                                                 {"userName", "User"}
                                         }),
                    jwt::params::secret(s.secret())
            };
            jwtToken.add_claim("exp", now);

            headers = SimpleWeb::CaseInsensitiveMultimap();
            headers.emplace("Authorization", jwtToken.signature());

            BOOST_CHECK_NO_THROW(svr->isAuthorized(headers));

            // Compare the json return value from isAuthorized for a valid secret
            BOOST_CHECK_EQUAL(svr->isAuthorized(headers)->payload(), nlohmann::json::object({
                {"exp", std::chrono::duration_cast<std::chrono::seconds>(now.time_since_epoch()).count()},
                {"userId", "5"},
                {"userName", "User"}
            }));

            BOOST_CHECK_EQUAL(svr->isAuthorized(headers)->secret().secret(), s.secret());
        }

        // Try using one more invalid secret
        jwtToken = {
                jwt::params::algorithm("HS256"),
                jwt::params::payload({
                                             {"userId", "5"},
                                             {"userName", "User"}
                                     }),
                jwt::params::secret(svr->getvJwtSecrets()->at(0).secret() + "notreal")
        };
        jwtToken.add_claim("exp", now);

        headers = SimpleWeb::CaseInsensitiveMultimap();
        headers.emplace("Authorization", jwtToken.signature());
        BOOST_CHECK_THROW(svr->isAuthorized(headers), eNotAuthorized);
    }
BOOST_AUTO_TEST_SUITE_END();