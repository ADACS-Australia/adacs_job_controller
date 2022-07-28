FROM ubuntu:jammy AS build_base

# Update the container and install the required packages
ENV DEBIAN_FRONTEND="noninteractive"

# Install ca-certificates
RUN apt-get update && apt-get install -y ca-certificates

# Switch mirror
RUN echo "deb mirror://mirrors.ubuntu.com/mirrors.txt jammy main restricted universe multiverse" > /etc/apt/sources.list && \
    echo "deb mirror://mirrors.ubuntu.com/mirrors.txt jammy-updates main restricted universe multiverse" >> /etc/apt/sources.list && \
    echo "deb mirror://mirrors.ubuntu.com/mirrors.txt jammy-security main restricted universe multiverse" >> /etc/apt/sources.list

RUN apt-get update && apt-get -y dist-upgrade && apt-get -y install python3 python3-venv gcovr mariadb-client libunwind-dev libdw-dev libgtest-dev libmysqlclient-dev build-essential cmake libboost-dev libgoogle-glog-dev libboost-test-dev libboost-system-dev libboost-thread-dev libboost-coroutine-dev libboost-context-dev libssl-dev libboost-filesystem-dev libboost-program-options-dev libboost-regex-dev libevent-dev libfmt-dev libdouble-conversion-dev libcurl4-openssl-dev git libjemalloc-dev libzstd-dev liblz4-dev libsnappy-dev libbz2-dev valgrind libdwarf-dev clang-tidy

# Copy in the source directory
ADD src /src

# Set up the build directory and configure the project with cmake
RUN mkdir /src/build
WORKDIR /src/build
RUN cmake -DCMAKE_BUILD_TYPE=Debug ..

# Build dependencies
RUN cmake --build . --target folly -- -j `nproc`
RUN cmake --build . --target folly_exception_tracer -- -j `nproc`
RUN cmake --build . --target folly_exception_counter -- -j `nproc`


FROM build_base AS build_production

# Build the production server
RUN cmake --build . --target gwcloud_job_server -- -j `nproc`


FROM build_base AS build_tests

# Build the test server and save the clang-tidy output
RUN cmake --build . --target Boost_Tests_run -- -j `nproc` 2> tidy.txt


FROM ubuntu:jammy AS production

ENV DEBIAN_FRONTEND=noninteractive

# Install runtime dependencies
RUN apt-get update && apt-get -y dist-upgrade && apt-get -y install python3 python3-venv tzdata libdw1 libboost-filesystem1.74.0  libdouble-conversion3 libgflags2.2 libgoogle-glog0v5 libmysqlclient21 libfmt8 

# Set the timezone
ENV TZ=Australia/Melbourne
RUN ln -snf /usr/share/zoneinfo/$TZ /etc/localtime && echo $TZ > /etc/timezone
RUN dpkg-reconfigure --frontend noninteractive tzdata

# Create jobserver user
RUN useradd -r jobserver

# Create the jobserver directory
RUN mkdir /jobserver

# Set the working directory
WORKDIR /jobserver

# Copy the schema and keyserver 
ADD src/utils /jobserver/utils
RUN chown -R jobserver:jobserver /jobserver

# Set the user to the jobserver user
USER jobserver

# Set up the keyserver venv
RUN python3 -m venv /jobserver/utils/keyserver/venv
RUN /jobserver/utils/keyserver/venv/bin/pip install wheel
RUN /jobserver/utils/keyserver/venv/bin/pip install -r /jobserver/utils/keyserver/requirements.txt

# Set up the schema venv
RUN python3 -m venv /jobserver/utils/schema/venv
RUN /jobserver/utils/schema/venv/bin/pip install wheel
RUN /jobserver/utils/schema/venv/bin/pip install -r /jobserver/utils/schema/requirements.txt

# Make sure that the logs directory exists
RUN mkdir /jobserver/logs

# Clean up apt
USER root
RUN rm -rf /var/lib/apt/lists/

# Copy the job server binary in and set permissions
COPY --from=build_production /src/build/gwcloud_job_server ./
RUN chmod +x gwcloud_job_server

# Copy the run script
ADD ./docker/scripts/runserver.sh /runserver.sh
RUN chmod +x /runserver.sh

USER jobserver

# Expose the ports and set the entrypoint
EXPOSE 8000 8001
CMD /runserver.sh


FROM build_tests AS test

WORKDIR /src

# Set up the schema venv
RUN python3 -m venv utils/schema/venv
RUN utils/schema/venv/bin/pip install wheel
RUN utils/schema/venv/bin/pip install -r utils/schema/requirements.txt

# Install required packages for testing
RUN apt-get install -y gcovr mariadb-client

# Copy the run script
ADD ./docker/scripts/runtests.sh /runtests.sh
ADD ./docker/scripts/runvalgrind.sh /runvalgrind.sh
RUN chmod +x /runtests.sh
RUN chmod +x /runvalgrind.sh

# We run tests as root, not as jobserver

# Set the entrypoint
CMD /runtests.sh
