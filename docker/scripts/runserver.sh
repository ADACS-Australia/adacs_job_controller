#! /bin/bash
# /jobserver is already the working directory (set in dockerfile)

# Ensure the logs directory is writable by jobserver user
mkdir -p /jobserver/logs
chmod 755 /jobserver/logs
touch /jobserver/logs/logfile
chown -R jobserver:jobserver /jobserver/logs

# Wait for MySQL to be ready (using TCP connection check instead of mysqladmin)
echo "Waiting for MySQL to be ready..."
until nc -z "${DATABASE_HOST}" 3306 2>/dev/null; do
    echo "MySQL is unavailable - sleeping"
    sleep 2
done
echo "MySQL port is open - waiting for it to be fully ready..."
sleep 2
echo "MySQL is up - continuing"

# Migrate the database (as jobserver user)
su -s /bin/bash jobserver -c "utils/schema/venv/bin/python utils/schema/manage.py migrate"

# Run the jobserver (as jobserver user)
su -s /bin/bash jobserver -c "./adacs_job_controller 2>&1 | tee ./logs/logfile"
