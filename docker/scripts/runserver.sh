#! /bin/bash
# /jobserver is already the working directory (set in dockerfile)

# Wait for MySQL to be ready
echo "Waiting for MySQL to be ready..."
until mysqladmin ping -h "${DATABASE_HOST}" --silent 2>&1 | grep -q "mysqld is alive"; do
    echo "MySQL is unavailable - sleeping"
    sleep 2
done
echo "MySQL is up - continuing"

# Migrate the database
utils/schema/venv/bin/python utils/schema/manage.py migrate;

# Run the jobserver
./adacs_job_controller 2>&1 | tee ./logs/logfile
