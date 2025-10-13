#!/bin/bash

# Create .env from template if it doesn't exist
if [ ! -f .env ]; then
    cp .env.template .env
fi

export DOCKER_BUILDKIT=1

# Standard build of the normal docker-compose file
docker-compose -f docker/docker-compose.yaml build
