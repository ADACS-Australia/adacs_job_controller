//
// Created by lewis on 8/17/22.
//

#include "../../tests/fixtures/DatabaseFixture.h"
#include "../../tests/fixtures/WebSocketClientFixture.h"

struct PingPongTestDataFixture : public DatabaseFixture, public WebSocketClientFixture {
    uint64_t jobId;
    bool bReady = false;
    bool bReceivedPing = false;
    bool bClosed = false;
    std::chrono::time_point<std::chrono::system_clock> zeroTime = {};

    PingPongTestDataFixture() {
        // Fabricate data
        // Create a job to request file lists for
        jobId = database->run(
                insert_into(jobTable)
                        .set(
                                jobTable.user = 1,
                                jobTable.parameters = "params1",
                                jobTable.cluster = httpServer->getvJwtSecrets()->at(2).clusters()[0],
                                jobTable.bundle = "my_test_bundle",
                                jobTable.application = httpServer->getvJwtSecrets()->at(0).name()
                        )
        );

        websocketClient->on_message = [&](auto connection, auto in_message) {
            onWebsocketMessage(connection, in_message);
        };

        websocketClient->on_ping = [&](auto) {
            bReceivedPing = true;
        };

        websocketClient->on_close = [&](auto, auto, auto) {
            bClosed = true;
        };

        websocketClient->on_error = [&](auto, auto) {
            bClosed = true;
        };

        startWebSocketClient();

        // Wait for the client to connect
        while (!bReady) {
            // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
            std::this_thread::sleep_for(std::chrono::milliseconds(10));
        }
    }

    void onWebsocketMessage([[maybe_unused]] auto connection, auto in_message) {
        auto data = in_message->string();
        Message msg(std::vector<uint8_t>(data.begin(), data.end()));

        // Ignore the ready message
        if (msg.getId() == SERVER_READY) {
            bReady = true;
            return;
        }
    };

    void runCheckPings() {
        bReceivedPing = false;

        clusterManager->callcheckPings();

        while (!bReceivedPing && !bClosed) {
            // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
            std::this_thread::sleep_for(std::chrono::milliseconds(1));
        }

        // Wait a moment for the pong to be processed
        // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
        std::this_thread::sleep_for(std::chrono::milliseconds(100));
    }
};

BOOST_FIXTURE_TEST_SUITE(ping_pong_test_suite, PingPongTestDataFixture)
    BOOST_AUTO_TEST_CASE(test_mClusterPing_has_correct_entry_after_connection) {
        // Check that the default setup for the mClusterPings is correct
        BOOST_CHECK_EQUAL(clusterManager->getmClusterPings()->size(), 1);
        BOOST_CHECK_EQUAL(clusterManager->getmClusterPings()->begin()->first, *clusterManager->getvClusters()->at(0)->getpConnection());
        BOOST_CHECK_MESSAGE(clusterManager->getmClusterPings()->begin()->second.pingTimestamp == zeroTime, "pingTimestamp was not zero when it should have been");
        BOOST_CHECK_MESSAGE(clusterManager->getmClusterPings()->begin()->second.pongTimestamp == zeroTime, "pongTimestamp was not zero when it should have been");
    }

    BOOST_AUTO_TEST_CASE(test_checkPings_send_ping_success) {
        // First check that when checkPings is called for the first time after a cluster has connected
        // that a ping is sent, and a pong received
        runCheckPings();

        // Check that neither ping or pong timestamp is zero
        BOOST_CHECK_MESSAGE(clusterManager->getmClusterPings()->begin()->second.pingTimestamp != zeroTime, "pingTimestamp was zero when it should not have been");
        BOOST_CHECK_MESSAGE(clusterManager->getmClusterPings()->begin()->second.pongTimestamp != zeroTime, "pongTimestamp was zero when it should not have been");

        // The cluster should still be connected
        BOOST_CHECK_EQUAL(clusterManager->getmClusterPings()->size(), 1);
        BOOST_CHECK_EQUAL(clusterManager->getmConnectedClusters()->size(), 1);

        auto previousPingTimestamp = clusterManager->getmClusterPings()->begin()->second.pingTimestamp;
        auto previousPongTimestamp = clusterManager->getmClusterPings()->begin()->second.pongTimestamp;

        // Run the ping pong again, the new ping/pong timestamps should be greater than the previous ones
        runCheckPings();

        // Check that neither ping or pong timestamp is zero
        BOOST_CHECK_MESSAGE(clusterManager->getmClusterPings()->begin()->second.pingTimestamp > previousPingTimestamp, "pingTimestamp was not greater than the previous ping timestamp when it should not have been");
        BOOST_CHECK_MESSAGE(clusterManager->getmClusterPings()->begin()->second.pongTimestamp > previousPongTimestamp, "pongTimestamp was not greater than the previous pong timestamp when it should not have been");

        // The cluster should still be connected
        BOOST_CHECK_EQUAL(clusterManager->getmConnectedClusters()->size(), 1);
    }

    BOOST_AUTO_TEST_CASE(test_checkPings_handle_zero_time) {
        // If checkPings is called, and the pongTimestamp is zero, then the connection should be disconnected. This
        // case indicates that the remote end never responded to the ping, or did not respond to the ping in
        // a timely manner (indicating a communication problem)
        runCheckPings();

        // Set the pongTimeout back to zero
        clusterManager->getmClusterPings()->begin()->second.pongTimestamp = zeroTime;

        // Running checkPings should now terminate the connection
        runCheckPings();

        // Wait for the websocket to be closed
        while (!bClosed) {
            // NOLINTNEXTLINE(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
            std::this_thread::sleep_for(std::chrono::milliseconds(1));
        }

        BOOST_CHECK_EQUAL(clusterManager->getmClusterPings()->size(), 0);
        BOOST_CHECK_EQUAL(clusterManager->getmConnectedClusters()->size(), 0);
    }
BOOST_AUTO_TEST_SUITE_END()
  