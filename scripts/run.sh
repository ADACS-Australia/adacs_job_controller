#!/bin/bash
set -e

# Check if .env file exists
if [ ! -f .env ]; then
    echo "Error: .env file not found!"
    echo "Please copy .env.template to .env and configure it with your settings:"
    echo "  cp .env.template .env"
    echo "  # Then edit .env with your configuration"
    exit 1
fi

export DOCKER_BUILDKIT=1

# Run docker-compose in detached mode
docker-compose -f docker/docker-compose.yaml up -d

echo ""
echo "Job Controller is starting..."
echo "  - HTTP API: http://localhost:8000"
echo "  - WebSocket: ws://localhost:8001"
echo ""
echo "To view logs: docker-compose -f docker/docker-compose.yaml logs -f"
echo "To stop: docker-compose -f docker/docker-compose.yaml down"

