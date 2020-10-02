#!/bin/bash

# Reset the test_report directory
rm -Rf ./test_report
mkdir ./test_report

# Test build using docker-compose override file
docker-compose -f docker/docker-compose.yaml -f docker/docker-compose.test.yaml build
docker-compose -f docker/docker-compose.yaml -f docker/docker-compose.test.yaml run web /runtests.sh
docker-compose -f docker/docker-compose.yaml -f docker/docker-compose.test.yaml rm -fs db
docker volume rm docker_var_lib_mysql_job_server_test
