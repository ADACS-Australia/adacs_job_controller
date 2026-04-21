# Multi-stage Dockerfile for ADACS Job Controller (Rust)
# Stages: build → test → production

# ============================================================
# Stage 1: Builder - compile the Rust binary
# ============================================================
FROM rust:1.87-bookworm AS builder

WORKDIR /app

# Install MySQL client dev libs for sqlx
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    default-libmysqlclient-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy manifests first for dependency caching
COPY Cargo.toml Cargo.lock ./

# Create a dummy main to build dependencies
RUN mkdir src && echo "fn main() {}" > src/main.rs && \
    cargo build --release 2>/dev/null || true && \
    rm -rf src

# Copy actual source
COPY src/ src/
COPY tests/ tests/
COPY migrations/ migrations/

# Build release binary
RUN cargo build --release

# ============================================================
# Stage 2: Test image - for CI testing
# ============================================================
FROM builder AS test

# Install test dependencies
RUN rustup component add llvm-tools-preview && \
    cargo install cargo-llvm-cov cargo-nextest

# Install runtime deps for integration tests
RUN apt-get update && apt-get install -y --no-install-recommends \
    default-mysql-client \
    netcat-openbsd \
    python3 \
    python3-pip \
    python3-venv \
    && rm -rf /var/lib/apt/lists/*

COPY docker/scripts/ /app/docker/scripts/
RUN chmod +x /app/docker/scripts/*.sh

CMD ["/app/docker/scripts/runtests.sh"]

# ============================================================
# Stage 3: Production image - minimal runtime
# ============================================================
FROM debian:bookworm-slim AS production

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libssl3 \
    default-mysql-client \
    netcat-openbsd \
    openssh-client \
    python3 \
    python3-pip \
    python3-venv \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN groupadd -r jobserver && useradd -r -g jobserver -m jobserver

WORKDIR /app

# Copy binary
COPY --from=builder /app/target/release/adacs_job_controller /app/adacs_job_controller

# Copy runtime scripts and utilities
COPY docker/scripts/ /app/docker/scripts/
RUN chmod +x /app/docker/scripts/*.sh

# Set up Python keyserver (if needed)
COPY --chown=jobserver:jobserver utils/ /app/utils/ 2>/dev/null || true

# Expose HTTP and WebSocket ports
EXPOSE 8000 8001

ENTRYPOINT ["/app/docker/scripts/runserver.sh"]
