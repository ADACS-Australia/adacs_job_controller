from ubuntu:focal

# Update the container and install the required packages
ENV DEBIAN_FRONTEND="noninteractive"

RUN apt-get update
RUN apt-get -y dist-upgrade
RUN apt-get -y install rsync sudo python3-dev python3-virtualenv libunwind-dev libdw-dev libgtest-dev libmysqlclient-dev build-essential cmake libboost-dev libgoogle-glog-dev libboost-test-dev libboost-system-dev libboost-thread-dev libboost-coroutine-dev libboost-context-dev libssl-dev libboost-filesystem-dev libboost-program-options-dev libboost-regex-dev libevent-dev libfmt-dev libdouble-conversion-dev libcurl4-openssl-dev git

# Copy in the source directory
COPY src /src

# Build the job server
RUN mkdir /src/build
WORKDIR /src/build
RUN cmake -DCMAKE_BUILD_TYPE=Debug .. || true
RUN cmake --build . --target gwcloud_job_server -- -j `grep -c ^processor /proc/cpuinfo`

# Create the install directory and copy the job server in
RUN mkdir -p /jobserver
RUN cp gwcloud_job_server /jobserver/
RUN mkdir /jobserver/logs
RUN mkdir -p /jobserver/utils/keyserver/

# Set up the keyserver venv
RUN cp /src/utils/keyserver/keyserver.py /jobserver/utils/keyserver/
RUN rm -Rf /jobserver/utils/keyserver/venv
RUN virtualenv -p python3 /jobserver/utils/keyserver/venv
RUN /jobserver/utils/keyserver/venv/bin/pip install -r /src/utils/keyserver/requirements.txt

# Set up the schema project
RUN rsync -arv /src/utils/schema /jobserver/utils/
RUN rm -Rf /jobserver/utils/schema/venv
RUN virtualenv -p python3 /jobserver/utils/schema/venv
RUN /jobserver/utils/schema/venv/bin/pip install -r /src/utils/schema/requirements.txt

# Clean up
RUN rm -Rf /src
RUN apt-get remove --purge -y build-essential git cmake rsync
RUN apt-get autoremove -y --purge

# Create jobserver user
RUN useradd -r jobserver

# Set permissions for the job server
RUN chown -R jobserver:jobserver /jobserver

WORKDIR /jobserver

COPY ./docker/scripts/runserver.sh /runserver.sh
RUN chmod +x /runserver.sh

EXPOSE 8000
EXPOSE 8001
CMD sudo -E -u jobserver /runserver.sh
