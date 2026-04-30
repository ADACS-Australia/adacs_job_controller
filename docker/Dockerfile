# Multi-stage Dockerfile for ADACS Job Controller (Rust)
# Stages: builder → production

# ============================================================
# Stage 1: Builder - compile the Rust binary
# ============================================================
FROM debian:bookworm-slim AS builder

WORKDIR /app

# Install build dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    curl \
    gcc \
    pkg-config \
    libssl-dev \
    default-libmysqlclient-dev \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Install Rust via rustup (version pinned by rust-toolchain.toml)
ENV CARGO_HOME=/root/.cargo
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal && \
    . "$CARGO_HOME/env" && \
    rustc --version

ENV PATH="/root/.cargo/bin:$PATH"

# Copy manifests first for dependency caching
COPY src/Cargo.toml src/Cargo.lock src/rust-toolchain.toml ./

# Create a dummy main to build dependencies
RUN mkdir src && echo "fn main() {}" > src/main.rs && \
    cargo build --release 2>/dev/null || true && \
    rm -rf src

# Copy actual source
COPY src/src/ src/
COPY src/tests/ tests/

# Build release binary
RUN cargo build --release

# ============================================================
# Stage 2: Production image - minimal runtime
# ============================================================
FROM debian:bookworm-slim AS production

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libssl3 \
    netcat-openbsd \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN groupadd -r jobserver && useradd -r -g jobserver -m jobserver

WORKDIR /app

# Copy binary
COPY --from=builder /app/target/release/adacs_job_controller /app/adacs_job_controller

# Copy runtime scripts
COPY docker/scripts/ /app/docker/scripts/
RUN chmod +x /app/docker/scripts/*.sh

# Expose HTTP and WebSocket ports
EXPOSE 8000 8001

ENTRYPOINT ["/app/docker/scripts/runserver.sh"]
