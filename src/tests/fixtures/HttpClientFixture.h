//
// Created by lewis on 7/25/22.
//

#ifndef GWCLOUD_JOB_SERVER_HTTPCLIENTFIXTURE_H
#define GWCLOUD_JOB_SERVER_HTTPCLIENTFIXTURE_H

#include <nlohmann/json.hpp>

#include "../utils.h"

struct HttpClientFixture
{
    TestHttpClient httpClient = TestHttpClient("localhost:8000");
    nlohmann::json jsonResult;
    nlohmann::json jsonParams;
};

#endif  // GWCLOUD_JOB_SERVER_HTTPCLIENTFIXTURE_H
