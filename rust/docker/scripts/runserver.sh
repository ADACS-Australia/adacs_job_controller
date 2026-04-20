#!/bin/bash
set -e

# Ensure logs directory exists
mkdir -p /app/logs

# Wait for MySQL to be available
echo "Waiting for MySQL..."
until nc -z "${DATABASE_HOST:-db}" "${DATABASE_PORT:-3306}"; do
    echo "  MySQL not ready, retrying in 2s..."
    sleep 2
done
echo "MySQL is ready."

# Run database migrations (Django schema is external)
# If sqlx migrations exist, run them
if [ -d "/app/migrations" ] && command -v sqlx &> /dev/null; then
    echo "Running database migrations..."
    DATABASE_URL="mysql://${MYSQL_USER:-jobserver}:${MYSQL_PASSWORD:-jobserver}@${DATABASE_HOST:-db}:${DATABASE_PORT:-3306}/${MYSQL_DATABASE:-jobserver}" \
        sqlx migrate run --source /app/migrations
fi

echo "Starting ADACS Job Controller..."
exec su -s /bin/bash jobserver -c "/app/adacs_job_controller 2>&1 | tee /app/logs/logfile"
