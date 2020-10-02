from ubuntu:focal

# Update the container and install the required packages
ENV DEBIAN_FRONTEND="noninteractive"

RUN apt-get update
RUN apt-get -y dist-upgrade
RUN apt-get -y install rsync sudo python3-dev python3-virtualenv libunwind-dev libdw-dev libgtest-dev libmysqlclient-dev build-essential cmake libboost-dev libgoogle-glog-dev libboost-test-dev libboost-system-dev libboost-thread-dev libboost-coroutine-dev libboost-context-dev libssl-dev libboost-filesystem-dev libboost-program-options-dev libboost-regex-dev libevent-dev libfmt-dev libdouble-conversion-dev libcurl4-openssl-dev git gcovr mysql-client

# Copy in the source directory
COPY src /src

# Build the job server
RUN mkdir /src/build
WORKDIR /src/build
RUN cmake -DCMAKE_BUILD_TYPE=Debug .. || true
RUN cmake --build . --target Boost_Tests_run -- -j `grep -c ^processor /proc/cpuinfo`

# Set up the schema project
RUN rm -Rf /src/utils/schema/venv
RUN virtualenv -p python3 /src/utils/schema/venv
RUN /src/utils/schema/venv/bin/pip install -r /src/utils/schema/requirements.txt

# Create jobserver user
RUN useradd -r jobserver

# Set permissions for the job server
RUN chown -R jobserver:jobserver /src

WORKDIR /src

COPY ./docker/scripts/runtests.sh /runtests.sh
RUN chmod +x /runtests.sh

CMD sudo -E -u jobserver /runtests.sh
