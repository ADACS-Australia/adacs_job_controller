from ubuntu:eoan

# Update the container and install the required packages
RUN apt-get update
RUN apt-get -y dist-upgrade
RUN apt-get -y install rsync sudo python3-dev python-virtualenv libunwind-dev libdw-dev libgtest-dev libmysqlclient-dev build-essential cmake libboost-dev libgoogle-glog-dev libboost-test-dev libboost-system-dev libboost-thread-dev libboost-coroutine-dev libboost-context-dev libssl-dev libboost-filesystem-dev libboost-program-options-dev libboost-regex-dev libevent-dev libfmt-dev libdouble-conversion-dev libcurl4-openssl-dev git

# Copy in the source directory
COPY src /src

# Build the job server
RUN mkdir /src/build
WORKDIR /src/build
RUN cmake -DCMAKE_BUILD_TYPE=Debug .. || true
RUN cmake --build . --target gwcloud_job_server -- -j 6

# Create the install directory and copy the job server in
RUN mkdir -p /jobserver
RUN cp gwcloud_job_server /jobserver/
RUN mkdir /jobserver/logs

# Build the suid binary
RUN mkdir /src/utils/keyserver/build
WORKDIR /src/utils/keyserver/build
RUN cmake -DCMAKE_BUILD_TYPE=Debug ..
RUN cmake --build . --target keyserver -- -j 6

# Copy the keyserver
RUN mkdir -p /jobserver/utils/keyserver
RUN cp /src/utils/keyserver/build/keyserver /jobserver/utils/keyserver/

# Set up the keyserver venv
RUN cp /src/utils/keyserver/keyserver.py /jobserver/utils/keyserver/
RUN virtualenv -p python3 /jobserver/utils/keyserver/venv
RUN /jobserver/utils/keyserver/venv/bin/pip install -r /src/utils/keyserver/requirements.txt

# Set up the schema project
RUN rsync -arv /src/utils/schema /jobserver/utils/
RUN virtualenv -p python3 /jobserver/utils/schema/venv
RUN /jobserver/utils/schema/venv/bin/pip install -r /src/utils/schema/requirements.txt

# Clean up
RUN rm -Rf /src
RUN apt-get remove --purge -y build-essential git cmake rsync
RUN apt-get autoremove -y --purge

# Copy keys
COPY config/keys /jobserver/utils/keyserver/keys
COPY config/cluster_config.json /jobserver/utils/

# Create jobserver user
RUN useradd -r jobserver

# Set permissions for the job server
RUN chown -R jobserver:jobserver /jobserver/gwcloud_job_server
RUN chown -R jobserver:jobserver /jobserver/logs
RUN chown -R jobserver:jobserver /jobserver/utils/schema

# Set permissions for the keyserver
RUN chown -R root:root /jobserver/utils/keyserver
RUN chmod -R o=- /jobserver/utils/keyserver
RUN chmod o=x /jobserver/utils/keyserver
RUN chmod u+s /jobserver/utils/keyserver/keyserver
RUN chmod o+x /jobserver/utils/keyserver/keyserver

WORKDIR /jobserver

COPY ./runserver.sh /runserver.sh
RUN chmod +x /runserver.sh

EXPOSE 8000
EXPOSE 8001
CMD sudo -E -u jobserver /runserver.sh