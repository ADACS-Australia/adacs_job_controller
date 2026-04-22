//
// Created by lewis on 22/10/20.
//
#include <boost/test/unit_test.hpp>
import HandleFileList;

extern auto randomInt(uint64_t start, uint64_t end) -> uint64_t;


static const std::vector<sFile> fileListData = {
    {.fileName = "/", .fileSize = 0, .permissions = 0, .isDirectory = true},
    {.fileName = "/test", .fileSize = randomInt(0, static_cast<uint64_t>(-1)), .permissions = 0, .isDirectory = false},
    {.fileName = "/testdir", .fileSize = 0, .permissions = 0, .isDirectory = true},
    {.fileName    = "/testdir/file",
     .fileSize    = randomInt(0, static_cast<uint64_t>(-1)),
     .permissions = 0,
     .isDirectory = false},
    {.fileName    = "/testdir/file2",
     .fileSize    = randomInt(0, static_cast<uint64_t>(-1)),
     .permissions = 0,
     .isDirectory = false},
    {.fileName    = "/testdir/file3",
     .fileSize    = randomInt(0, static_cast<uint64_t>(-1)),
     .permissions = 0,
     .isDirectory = false},
    {.fileName = "/testdir/testdir1", .fileSize = 0, .permissions = 0, .isDirectory = true},
    {.fileName    = "/testdir/testdir1/file",
     .fileSize    = randomInt(0, static_cast<uint64_t>(-1)),
     .permissions = 0,
     .isDirectory = false},
    {.fileName = "/test2", .fileSize = randomInt(0, static_cast<uint64_t>(-1)), .permissions = 0, .isDirectory = false},
};

namespace {
auto checkMatch(std::vector<sFile> first, std::vector<sFile> second) -> bool
{
    if (first.size() != second.size())
    {
        return false;
    }


    for (size_t i = 0; i < first.size(); i++)
    {
        if (first[i].fileName != second[i].fileName || first[i].isDirectory != second[i].isDirectory ||
            first[i].permissions != second[i].permissions || first[i].fileSize != second[i].fileSize)
        {
            return false;
        }
    }

    return true;
}
}  // namespace

BOOST_AUTO_TEST_SUITE(filterFilesTestSuite)

BOOST_AUTO_TEST_CASE(test_relative_path_recursive)
{
    auto resultFiles = filterFiles(fileListData, "/testdir/..", true);
    BOOST_CHECK_MESSAGE(checkMatch(resultFiles, fileListData), "File list did not match when it should have");

    resultFiles = filterFiles(fileListData, "/testdir/../", true);
    BOOST_CHECK_MESSAGE(checkMatch(resultFiles, fileListData), "File list did not match when it should have");

    resultFiles = filterFiles(fileListData, "/testdir/../test2/not/real/../../../testdir/..", true);
    BOOST_CHECK_MESSAGE(checkMatch(resultFiles, fileListData), "File list did not match when it should have");

    resultFiles = filterFiles(fileListData, "/testdir/../test2/not/real/../../../testdir/../", true);
    BOOST_CHECK_MESSAGE(checkMatch(resultFiles, fileListData), "File list did not match when it should have");


    std::vector<sFile> expected =
        {fileListData[2], fileListData[3], fileListData[4], fileListData[5], fileListData[6], fileListData[7]};


    resultFiles = filterFiles(fileListData, "/testdir/../test2/not/real/../../../testdir", true);
    BOOST_CHECK_MESSAGE(checkMatch(resultFiles, expected), "File list did not match when it should have");

    resultFiles = filterFiles(fileListData, "/testdir/../test2/not/real/../../../testdir/", true);
    BOOST_CHECK_MESSAGE(checkMatch(resultFiles, expected), "File list did not match when it should have");
}

BOOST_AUTO_TEST_CASE(test_absolute_path_recursive)
{
    auto resultFiles = filterFiles(fileListData, "", true);
    BOOST_CHECK_MESSAGE(checkMatch(resultFiles, fileListData), "File list did not match when it should have");

    resultFiles = filterFiles(fileListData, "/", true);
    BOOST_CHECK_MESSAGE(checkMatch(resultFiles, fileListData), "File list did not match when it should have");


    std::vector<sFile> expected =
        {fileListData[2], fileListData[3], fileListData[4], fileListData[5], fileListData[6], fileListData[7]};


    resultFiles = filterFiles(fileListData, "/testdir", true);
    BOOST_CHECK_MESSAGE(checkMatch(resultFiles, expected), "File list did not match when it should have");

    resultFiles = filterFiles(fileListData, "/testdir/", true);
    BOOST_CHECK_MESSAGE(checkMatch(resultFiles, expected), "File list did not match when it should have");


    expected = {fileListData[6], fileListData[7]};


    resultFiles = filterFiles(fileListData, "/testdir/testdir1", true);
    BOOST_CHECK_MESSAGE(checkMatch(resultFiles, expected), "File list did not match when it should have");

    resultFiles = filterFiles(fileListData, "/testdir/testdir1/", true);
    BOOST_CHECK_MESSAGE(checkMatch(resultFiles, expected), "File list did not match when it should have");

    resultFiles = filterFiles(fileListData, "testdir/testdir1", true);
    BOOST_CHECK_MESSAGE(checkMatch(resultFiles, expected), "File list did not match when it should have");

    resultFiles = filterFiles(fileListData, "testdir/testdir1/", true);
    BOOST_CHECK_MESSAGE(checkMatch(resultFiles, expected), "File list did not match when it should have");
}

BOOST_AUTO_TEST_CASE(test_absolute_path_non_recursive)
{
    std::vector<sFile> expected = {fileListData[0], fileListData[1], fileListData[2], fileListData[8]};


    auto resultFiles = filterFiles(fileListData, "", false);
    BOOST_CHECK_MESSAGE(checkMatch(resultFiles, expected), "File list did not match when it should have");

    resultFiles = filterFiles(fileListData, "/", false);
    BOOST_CHECK_MESSAGE(checkMatch(resultFiles, expected), "File list did not match when it should have");


    expected = {fileListData[2], fileListData[3], fileListData[4], fileListData[5], fileListData[6]};


    resultFiles = filterFiles(fileListData, "/testdir", false);
    BOOST_CHECK_MESSAGE(checkMatch(resultFiles, expected), "File list did not match when it should have");

    resultFiles = filterFiles(fileListData, "/testdir/", false);
    BOOST_CHECK_MESSAGE(checkMatch(resultFiles, expected), "File list did not match when it should have");

    resultFiles = filterFiles(fileListData, "testdir", false);
    BOOST_CHECK_MESSAGE(checkMatch(resultFiles, expected), "File list did not match when it should have");

    resultFiles = filterFiles(fileListData, "testdir/", false);
    BOOST_CHECK_MESSAGE(checkMatch(resultFiles, expected), "File list did not match when it should have");


    expected = {fileListData[6], fileListData[7]};


    resultFiles = filterFiles(fileListData, "/testdir/testdir1", false);
    BOOST_CHECK_MESSAGE(checkMatch(resultFiles, expected), "File list did not match when it should have");

    resultFiles = filterFiles(fileListData, "/testdir/testdir1/", false);
    BOOST_CHECK_MESSAGE(checkMatch(resultFiles, expected), "File list did not match when it should have");

    resultFiles = filterFiles(fileListData, "testdir/testdir1", false);
    BOOST_CHECK_MESSAGE(checkMatch(resultFiles, expected), "File list did not match when it should have");

    resultFiles = filterFiles(fileListData, "testdir/testdir1/", false);
    BOOST_CHECK_MESSAGE(checkMatch(resultFiles, expected), "File list did not match when it should have");
}
BOOST_AUTO_TEST_SUITE_END()