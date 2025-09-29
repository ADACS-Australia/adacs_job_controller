//
// Created by lewis on 22/10/20.
//
#include <boost/test/unit_test.hpp>
import HandleFileList;

extern auto randomInt(uint64_t start, uint64_t end) -> uint64_t;


BOOST_AUTO_TEST_SUITE(filterFilesTestSuite)
const std::vector<sFile> fileListData = {
    // NOLINT(cert-err58-cpp)
    {"/", 0, 0, true},
    {"/test", randomInt(0, static_cast<uint64_t>(-1)), 0, false},
    {"/testdir", 0, 0, true},
    {"/testdir/file", randomInt(0, static_cast<uint64_t>(-1)), 0, false},
    {"/testdir/file2", randomInt(0, static_cast<uint64_t>(-1)), 0, false},
    {"/testdir/file3", randomInt(0, static_cast<uint64_t>(-1)), 0, false},
    {"/testdir/testdir1", 0, 0, true},
    {"/testdir/testdir1/file", randomInt(0, static_cast<uint64_t>(-1)), 0, false},
    {"/test2", randomInt(0, static_cast<uint64_t>(-1)), 0, false},
};

auto checkMatch(std::vector<sFile> first, std::vector<sFile> second) -> bool
{
    if (first.size() != second.size())
    {
        return false;
    }

    for (auto i = 0; i < first.size(); i++)
    {
        if (first[i].fileName != second[i].fileName || first[i].isDirectory != second[i].isDirectory ||
            first[i].permissions != second[i].permissions || first[i].fileSize != second[i].fileSize)
        {
            return false;
        }
    }

    return true;
}

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

    // NOLINTBEGIN(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
    std::vector<sFile> expected =
        {fileListData[2], fileListData[3], fileListData[4], fileListData[5], fileListData[6], fileListData[7]};
    // NOLINTEND(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)

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

    // NOLINTBEGIN(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
    std::vector<sFile> expected =
        {fileListData[2], fileListData[3], fileListData[4], fileListData[5], fileListData[6], fileListData[7]};
    // NOLINTEND(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)

    resultFiles = filterFiles(fileListData, "/testdir", true);
    BOOST_CHECK_MESSAGE(checkMatch(resultFiles, expected), "File list did not match when it should have");

    resultFiles = filterFiles(fileListData, "/testdir/", true);
    BOOST_CHECK_MESSAGE(checkMatch(resultFiles, expected), "File list did not match when it should have");

    // NOLINTBEGIN(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
    expected = {fileListData[6], fileListData[7]};
    // NOLINTEND(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)

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
    // NOLINTBEGIN(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
    std::vector<sFile> expected = {fileListData[0], fileListData[1], fileListData[2], fileListData[8]};
    // NOLINTEND(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)

    auto resultFiles = filterFiles(fileListData, "", false);
    BOOST_CHECK_MESSAGE(checkMatch(resultFiles, expected), "File list did not match when it should have");

    resultFiles = filterFiles(fileListData, "/", false);
    BOOST_CHECK_MESSAGE(checkMatch(resultFiles, expected), "File list did not match when it should have");

    // NOLINTBEGIN(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
    expected = {fileListData[2], fileListData[3], fileListData[4], fileListData[5], fileListData[6]};
    // NOLINTEND(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)

    resultFiles = filterFiles(fileListData, "/testdir", false);
    BOOST_CHECK_MESSAGE(checkMatch(resultFiles, expected), "File list did not match when it should have");

    resultFiles = filterFiles(fileListData, "/testdir/", false);
    BOOST_CHECK_MESSAGE(checkMatch(resultFiles, expected), "File list did not match when it should have");

    resultFiles = filterFiles(fileListData, "testdir", false);
    BOOST_CHECK_MESSAGE(checkMatch(resultFiles, expected), "File list did not match when it should have");

    resultFiles = filterFiles(fileListData, "testdir/", false);
    BOOST_CHECK_MESSAGE(checkMatch(resultFiles, expected), "File list did not match when it should have");

    // NOLINTBEGIN(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)
    expected = {fileListData[6], fileListData[7]};
    // NOLINTEND(cppcoreguidelines-avoid-magic-numbers,readability-magic-numbers)

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