#include "utils.h"

std::default_random_engine rng;
bool bSeeded = false;

std::string getLastToken() {
    auto db = MySqlConnector();
    schema::JobserverClusteruuid clusterUuidTable;

    // Look up all cluster tokens
    auto uuidResults = db->run(
            select(all_of(clusterUuidTable))
                    .from(clusterUuidTable)
                    .unconditionally()
                    .order_by(clusterUuidTable.id.desc())
    );

    // Check that the uuid was valid
    if (uuidResults.empty())
        throw std::runtime_error("Couldn't get any cluster token?");

    // Return the uuid
    return uuidResults.front().uuid;
}

uint64_t randomInt(uint64_t start, uint64_t end) {
    if (!bSeeded) {
        bSeeded = true;
        rng.seed(std::chrono::system_clock::now().time_since_epoch().count());
    }

    std::uniform_int_distribution<uint64_t> rng_dist(start, end);
    return rng_dist(rng);
}

std::shared_ptr<std::vector<uint8_t>> generateRandomData(uint32_t count) {
    auto result = std::make_shared<std::vector<uint8_t>>();
    result->reserve(count);

    for (uint32_t i = 0; i < count; i++) {
        result->push_back(randomInt(0, 255));
    }

    return result;
}