#! /bin/bash
# /jobserver is already the working directory (set in dockerfile)

# Wait for MySQL to be ready (using TCP connection check instead of mysqladmin)
echo "Waiting for MySQL to be ready..."
until nc -z "${DATABASE_HOST}" 3306 2>/dev/null; do
    echo "MySQL is unavailable - sleeping"
    sleep 2
done
echo "MySQL port is open - waiting for it to be fully ready..."
sleep 2
echo "MySQL is up - continuing"

# Migrate the database
utils/schema/venv/bin/python utils/schema/manage.py migrate;

# Run the jobserver with unbuffered output
stdbuf -oL -eL ./adacs_job_controller 2>&1 | tee ./logs/logfile
