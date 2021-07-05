//
// Created by lewis on 22/10/20.
//
#include <boost/test/unit_test.hpp>
#include "../../Cluster/Cluster.h"
#include "../Utils/HandleFileList.h"

extern uint64_t randomInt(uint64_t start, uint64_t end);

BOOST_AUTO_TEST_SUITE(filterFilesTestSuite)
    std::vector<sFile> fileListData = {
            {"/", 0, 0, true},
            {"/test", randomInt(0, (uint64_t) -1), 0, false},
            {"/testdir", 0, 0, true},
            {"/testdir/file", randomInt(0, (uint64_t) -1), 0, false},
            {"/testdir/file2", randomInt(0, (uint64_t) -1), 0, false},
            {"/testdir/file3", randomInt(0, (uint64_t) -1), 0, false},
            {"/testdir/testdir1", 0, 0, true},
            {"/testdir/testdir1/file", randomInt(0, (uint64_t) -1), 0, false},
            {"/test2", randomInt(0, (uint64_t) -1), 0, false},
    };

    bool checkMatch(std::vector<sFile> a, std::vector<sFile> b) {
        if (a.size() != b.size())
            return false;

        for (auto i = 0; i < a.size(); i++) {
            if (
                    a[i].fileName != b[i].fileName
                    || a[i].isDirectory != b[i].isDirectory
                    || a[i].permissions != b[i].permissions
                    || a[i].fileSize != b[i].fileSize) {
                return false;
            }
        }

        return true;
    }

    BOOST_AUTO_TEST_CASE(test_relative_path_recursive) {
        auto r = filterFiles(fileListData, "/testdir/..", true);
        BOOST_CHECK_MESSAGE(checkMatch(r, fileListData), "File list did not match when it should have");

        r = filterFiles(fileListData, "/testdir/../", true);
        BOOST_CHECK_MESSAGE(checkMatch(r, fileListData), "File list did not match when it should have");

        r = filterFiles(fileListData, "/testdir/../test2/not/real/../../../testdir/..", true);
        BOOST_CHECK_MESSAGE(checkMatch(r, fileListData), "File list did not match when it should have");

        r = filterFiles(fileListData, "/testdir/../test2/not/real/../../../testdir/../", true);
        BOOST_CHECK_MESSAGE(checkMatch(r, fileListData), "File list did not match when it should have");

        std::vector<sFile> expected = {
                fileListData[2],
                fileListData[3],
                fileListData[4],
                fileListData[5],
                fileListData[6],
                fileListData[7]
        };

        r = filterFiles(fileListData, "/testdir/../test2/not/real/../../../testdir", true);
        BOOST_CHECK_MESSAGE(checkMatch(r, expected), "File list did not match when it should have");

        r = filterFiles(fileListData, "/testdir/../test2/not/real/../../../testdir/", true);
        BOOST_CHECK_MESSAGE(checkMatch(r, expected), "File list did not match when it should have");
    }

    BOOST_AUTO_TEST_CASE(test_absolute_path_recursive) {
        auto r = filterFiles(fileListData, "", true);
        BOOST_CHECK_MESSAGE(checkMatch(r, fileListData), "File list did not match when it should have");

        r = filterFiles(fileListData, "/", true);
        BOOST_CHECK_MESSAGE(checkMatch(r, fileListData), "File list did not match when it should have");

        std::vector<sFile> expected = {
                fileListData[2],
                fileListData[3],
                fileListData[4],
                fileListData[5],
                fileListData[6],
                fileListData[7]
        };

        r = filterFiles(fileListData, "/testdir", true);
        BOOST_CHECK_MESSAGE(checkMatch(r, expected), "File list did not match when it should have");

        r = filterFiles(fileListData, "/testdir/", true);
        BOOST_CHECK_MESSAGE(checkMatch(r, expected), "File list did not match when it should have");

        expected = {
                fileListData[6],
                fileListData[7]
        };

        r = filterFiles(fileListData, "/testdir/testdir1", true);
        BOOST_CHECK_MESSAGE(checkMatch(r, expected), "File list did not match when it should have");

        r = filterFiles(fileListData, "/testdir/testdir1/", true);
        BOOST_CHECK_MESSAGE(checkMatch(r, expected), "File list did not match when it should have");

        r = filterFiles(fileListData, "testdir/testdir1", true);
        BOOST_CHECK_MESSAGE(checkMatch(r, expected), "File list did not match when it should have");

        r = filterFiles(fileListData, "testdir/testdir1/", true);
        BOOST_CHECK_MESSAGE(checkMatch(r, expected), "File list did not match when it should have");
    }

    BOOST_AUTO_TEST_CASE(test_absolute_path_non_recursive) {
        std::vector<sFile> expected = {
                fileListData[0],
                fileListData[1],
                fileListData[2],
                fileListData[8]
        };

        auto r = filterFiles(fileListData, "", false);
        BOOST_CHECK_MESSAGE(checkMatch(r, expected), "File list did not match when it should have");

        r = filterFiles(fileListData, "/", false);
        BOOST_CHECK_MESSAGE(checkMatch(r, expected), "File list did not match when it should have");

        expected = {
                fileListData[2],
                fileListData[3],
                fileListData[4],
                fileListData[5],
                fileListData[6]
        };

        r = filterFiles(fileListData, "/testdir", false);
        BOOST_CHECK_MESSAGE(checkMatch(r, expected), "File list did not match when it should have");

        r = filterFiles(fileListData, "/testdir/", false);
        BOOST_CHECK_MESSAGE(checkMatch(r, expected), "File list did not match when it should have");

        r = filterFiles(fileListData, "testdir", false);
        BOOST_CHECK_MESSAGE(checkMatch(r, expected), "File list did not match when it should have");

        r = filterFiles(fileListData, "testdir/", false);
        BOOST_CHECK_MESSAGE(checkMatch(r, expected), "File list did not match when it should have");

        expected = {
                fileListData[6],
                fileListData[7]
        };

        r = filterFiles(fileListData, "/testdir/testdir1", false);
        BOOST_CHECK_MESSAGE(checkMatch(r, expected), "File list did not match when it should have");

        r = filterFiles(fileListData, "/testdir/testdir1/", false);
        BOOST_CHECK_MESSAGE(checkMatch(r, expected), "File list did not match when it should have");

        r = filterFiles(fileListData, "testdir/testdir1", false);
        BOOST_CHECK_MESSAGE(checkMatch(r, expected), "File list did not match when it should have");

        r = filterFiles(fileListData, "testdir/testdir1/", false);
        BOOST_CHECK_MESSAGE(checkMatch(r, expected), "File list did not match when it should have");
    }
BOOST_AUTO_TEST_SUITE_END()