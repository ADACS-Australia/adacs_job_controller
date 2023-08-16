#! /bin/bash
# /jobserver is already the working directory (set in dockerfile)

# Migrate the database
utils/schema/venv/bin/python utils/schema/manage.py migrate;

# Run the jobserver
./adacs_job_controller 2>&1 | tee ./logs/logfile
