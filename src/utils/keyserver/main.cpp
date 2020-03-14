#include <iostream>
#include <unistd.h>

using namespace std;

int main(int argc, char** argv) {
    // Check that the number of arguments is 2 + program name
    if (argc != 3)
        return 1;

    // Convert the arguments to strings
    auto sMachineName = string(argv[1]);
    auto sUUID = string(argv[2]);

    // Check for valid lengths
    if (sMachineName.length() > 50 || sUUID.length() != 36)
        return 1;

    // Check that the machine name only contains letters
    string validChars = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ1234567890-";
    if (sMachineName.find_first_not_of(validChars) != std::string::npos)
        return 1;

    // Check that the UUID is valid
    if (sUUID.find_first_not_of(validChars) != std::string::npos)
        return 1;

    // Change to root
    setuid( 0 );

    // Call the python script with the supplied parameters
    return system(("./venv/bin/python keyserver.py \"" + sMachineName + "\" \"" + sUUID + "\"").c_str());
}
