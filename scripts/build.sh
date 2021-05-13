#!/bin/bash

export DOCKER_BUILDKIT=1

# Standard build of the normal docker-compose file
docker-compose -f docker/docker-compose.yaml build
