#! /bin/bash
# /src is already the working directory (set in dockerfile)

# Wait for mysql to be ready
while ! mysqladmin ping -h"$DATABASE_HOST" --silent; do
    sleep 1
done

# Migrate the database
utils/schema/venv/bin/python utils/schema/manage.py migrate;

# Run the jobserver tests
build/Boost_Tests_run -catch_system_error=yes --log_format=JUNIT --show_progress=no --report_format=XML > /test_report/junit.xml

build/Boost_Tests_run --logger=HRF,all --color_output=true --report_format=HRF --show_progress=no

# Print some whitespace
printf "\n\n"

# Generate code coverage reports
gcovr -r . --xml-pretty -e "Lib/sqlpp11/" -e "Lib/sqlpp11-connector-mysql/" -e "Lib/json/" -e "Lib/folly/" -e "Lib/date/" -e "Lib/cpp-jwt/" -e "Lib/Simple-WebSocket-Server/" -e "Lib/Simple-Web-Server/" > /test_report/coverage.xml

gcovr -r . -e "Lib/sqlpp11/" -e "Lib/sqlpp11-connector-mysql/" -e "Lib/json/" -e "Lib/folly/" -e "Lib/date/" -e "Lib/cpp-jwt/" -e "Lib/Simple-WebSocket-Server/" -e "Lib/Simple-Web-Server/"

# Set full access on the test_report directory in case, so the host can delete the files if needed
chmod -R 666 /test_report
chmod 777 /test_report
