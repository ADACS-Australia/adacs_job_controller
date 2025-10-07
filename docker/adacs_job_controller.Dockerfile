FROM ubuntu:noble AS build_base

# Update the container and install the required packages
ENV DEBIAN_FRONTEND="noninteractive"

# Switch mirror to Australia (Ubuntu Noble uses .sources format)
RUN find /etc/apt/sources.list.d -name "*.sources" -exec sed --in-place --regexp-extended "s/(\/\/)(archive\.ubuntu)/\1au.\2/" {} \;

# Install wget and basic tools needed for LLVM script
RUN apt-get update && apt-get -y dist-upgrade && apt-get -y install wget lsb-release software-properties-common gnupg

# Install Clang 22 from official LLVM repository
RUN wget https://apt.llvm.org/llvm.sh && bash ./llvm.sh 22 all

# Install remaining build dependencies
RUN apt-get -y install python3 python3-venv gcovr mariadb-client libunwind-dev libdw-dev libgtest-dev libmysqlclient-dev build-essential cmake libboost-dev libgoogle-glog-dev libboost-test-dev libboost-system-dev libboost-thread-dev libboost-coroutine-dev libboost-context-dev libssl-dev libboost-filesystem-dev libboost-program-options-dev libboost-regex-dev libevent-dev libfmt-dev libdouble-conversion-dev libcurl4-openssl-dev git libjemalloc-dev libzstd-dev liblz4-dev libsnappy-dev libbz2-dev valgrind libdwarf-dev libfast-float-dev ninja-build libcpp-jwt-dev libhowardhinnant-date-dev nlohmann-json3-dev 

# Copy in the source directory
ADD src /src

# Set up the build directory and configure the project with cmake
RUN mkdir /src/build
WORKDIR /src/build
RUN cmake -G Ninja -DCMAKE_BUILD_TYPE=Debug ..

FROM build_base AS build_production

# Build the production server
RUN ninja adacs_job_controller


FROM build_base AS build_tests

# Build the test server and save the clang-tidy output
RUN ninja Boost_Tests_run 2> tidy.txt


FROM ubuntu:noble AS production

ENV DEBIAN_FRONTEND=noninteractive

# Install runtime dependencies
# Boost libraries required by folly (context, filesystem, program_options, regex, system, thread)
# and Simple-WebSocket-Server (system, thread, coroutine, context)
RUN apt-get update && apt-get -y dist-upgrade && apt-get -y install \
    python3 python3-venv tzdata netcat-openbsd \
    libdw1 libunwind8 libdwarf1 \
    libboost-filesystem1.83.0 libboost-system1.83.0 libboost-thread1.83.0 \
    libboost-coroutine1.83.0 libboost-context1.83.0 libboost-program-options1.83.0 \
    libboost-regex1.83.0 libboost-atomic1.83.0 \
    libdouble-conversion3 libgflags2.2 libgoogle-glog0v6t64 \
    libmysqlclient21 libfmt9 \
    build-essential libpython3-dev libffi-dev \
    libhowardhinnant-date-dev \
    libssl3t64 libcurl4 libevent-2.1-7t64 \
    libjemalloc2 libzstd1 liblz4-1 libsnappy1v5 libbz2-1.0

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
COPY --from=build_production /src/build/adacs_job_controller ./
RUN chmod +x adacs_job_controller

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
