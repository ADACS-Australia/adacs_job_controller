#!/bin/bash
set -e

# Ensure logs directory exists and is writable
mkdir -p /app/logs
chown -R jobserver:jobserver /app/logs

# Wait for MySQL to be available
echo "Waiting for MySQL..."
until nc -z "${DATABASE_HOST:-db}" "${DATABASE_PORT:-3306}"; do
    echo "  MySQL not ready, retrying in 2s..."
    sleep 2
done
echo "MySQL is ready."

echo "Starting ADACS Job Controller..."
export RUST_LOG="${RUST_LOG:-info}"
exec su -s /bin/bash jobserver -c "stdbuf -oL -eL /app/adacs_job_controller 2>&1 | stdbuf -oL tee /app/logs/logfile"
