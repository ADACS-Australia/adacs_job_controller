//
// Created by lewis on 7/25/22.
//

#ifndef GWCLOUD_JOB_SERVER_HTTPSERVERFIXTURE_H
#define GWCLOUD_JOB_SERVER_HTTPSERVERFIXTURE_H

#include "../../Cluster/ClusterManager.h"
#include "../utils.h"
#include <boost/test/unit_test.hpp>
#include <jwt/jwt.hpp>

struct HttpServerFixture {
    // NOLINTBEGIN(misc-non-private-member-variables-in-classes)
    const std::string sAccess = R"(
    [
        {
            "name": "app1",
            "secret": "super_secret1",
            "applications": [],
            "clusters": [
                "cluster2",
                "cluster3"
            ]
        },
        {
            "name": "app2",
            "secret": "super_secret2",
            "applications": [
                "app1"
            ],
            "clusters": [
                "cluster1"
            ]
        },
        {
            "name": "app3",
            "secret": "super_secret3",
            "applications": [
                "app1",
                "app2"
            ],
            "clusters": [
                "cluster1",
                "cluster2",
                "cluster3"
            ]
        },
        {
            "name": "app4",
            "secret": "super_secret4",
            "applications": [],
            "clusters": [
                "cluster1"
            ]
        }
    ]
    )";
    const std::string sClusters = R"(
    [
        {
            "name": "cluster1",
            "host": "cluster1.com",
            "username": "user1",
            "path": "/cluster1/",
            "key": "cluster1_key"
        },
        {
            "name": "cluster2",
            "host": "cluster2.com",
            "username": "user2",
            "path": "/cluster2/",
            "key": "cluster2_key"
        },
        {
            "name": "cluster3",
            "host": "cluster3.com",
            "username": "user3",
            "path": "/cluster3/",
            "key": "cluster3_key"
        }
    ]
    )";

    std::shared_ptr<ClusterManager> clusterManager;
    std::shared_ptr<HttpServer> httpServer;

    jwt::jwt_object jwtToken;
    // NOLINTEND(misc-non-private-member-variables-in-classes)

    HttpServerFixture() {
        // NOLINTBEGIN(concurrency-mt-unsafe)
        // Set up the test server
        setenv(CLUSTER_CONFIG_ENV_VARIABLE, base64Encode(sClusters).c_str(), 1);
        clusterManager = std::make_shared<ClusterManager>();

        setenv(ACCESS_SECRET_ENV_VARIABLE, base64Encode(sAccess).c_str(), 1);
        httpServer = std::make_shared<HttpServer>(clusterManager);
        // NOLINTEND(concurrency-mt-unsafe)

        // Start the http server
        httpServer->start();

        // Wait for the http server
        BOOST_CHECK_EQUAL(acceptingConnections(8000), true);
    }

    ~HttpServerFixture() {
        // Finished with the server
        httpServer->stop();
    }

    HttpServerFixture(HttpServerFixture const&) = delete;
    auto operator =(HttpServerFixture const&) -> HttpServerFixture& = delete;
    HttpServerFixture(HttpServerFixture&&) = delete;
    auto operator=(HttpServerFixture&&) -> HttpServerFixture& = delete;

    void setJwtSecret(auto secret) {
        // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
        auto timeNow = std::chrono::system_clock::now() + std::chrono::minutes{10};
        jwtToken = {
                jwt::params::algorithm("HS256"),
                jwt::params::payload({{"userName", "User"}}),
                jwt::params::secret(secret)
        };
        jwtToken.add_claim("exp", timeNow);

        // Since payload above only accepts string values, we need to set up any non-string values
        // separately
        // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
        jwtToken.payload().add_claim("userId", 5);
    }
};

#endif //GWCLOUD_JOB_SERVER_HTTPSERVERFIXTURE_H
