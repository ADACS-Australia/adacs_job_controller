#!/bin/bash

# Create .env from template if it doesn't exist
if [ ! -f .env ]; then
    cp .env.template .env
fi

export DOCKER_BUILDKIT=1

# Test build using docker-compose override file
docker-compose -f docker/docker-compose.yaml -f docker/docker-compose.valgrind.yaml build
docker-compose -f docker/docker-compose.yaml -f docker/docker-compose.valgrind.yaml run web /runvalgrind.sh

# Clean up
docker-compose -f docker/docker-compose.yaml -f docker/docker-compose.valgrind.yaml stop
docker-compose -f docker/docker-compose.yaml -f docker/docker-compose.valgrind.yaml down
docker-compose -f docker/docker-compose.yaml -f docker/docker-compose.valgrind.yaml rm -fs db
docker volume rm docker_var_lib_mysql_adacs_job_controller_valgrind
