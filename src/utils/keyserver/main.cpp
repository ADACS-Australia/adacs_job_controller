#include <iostream>
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

    // Call the python script with the supplied parameters
    return system(("./venv/bin/python keyserver.py \"" + sMachineName + "\" \"" + sUUID + "\"").c_str());
}
