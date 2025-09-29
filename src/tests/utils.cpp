#include "utils.h"

#include <sqlpp11/sqlpp11.h>

import jobserver_schema;
import MySqlConnector;

std::shared_ptr<std::default_random_engine> rng =
    nullptr;  // NOLINT(cppcoreguidelines-avoid-non-const-global-variables)

auto getLastToken() -> std::string
{
    auto database = MySqlConnector();
    schema::JobserverClusteruuid clusterUuidTable;

    // Look up all cluster tokens
    auto uuidResults = database->run(
        select(all_of(clusterUuidTable)).from(clusterUuidTable).unconditionally().order_by(clusterUuidTable.id.asc()));

    // Check that the uuid was valid
    if (uuidResults.empty())
    {
        throw std::runtime_error("Couldn't get any cluster token?");
    }

    // Return the uuid
    return uuidResults.front().uuid;
}

auto randomInt(uint64_t start, uint64_t end) -> uint64_t
{
    if (!rng)
    {
        rng = std::make_shared<std::default_random_engine>(std::chrono::system_clock::now().time_since_epoch().count());
    }

    std::uniform_int_distribution<uint64_t> rng_dist(start, end);
    return rng_dist(*rng);
}

auto generateRandomData(uint32_t count) -> std::shared_ptr<std::vector<uint8_t>>
{
    auto result = std::make_shared<std::vector<uint8_t>>();
    result->reserve(count);

    for (uint32_t i = 0; i < count; i++)
    {
        result->push_back(randomInt(0, std::numeric_limits<uint8_t>::max()));
    }

    return result;
}