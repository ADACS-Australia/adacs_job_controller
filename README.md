# ADACS Job Controller Server (Rust)

Rust port of the ADACS Job Controller Server. The original C++ implementation is in `legacy/`.

A server that manages distributed job submissions to HPC/compute clusters via an HTTP REST API and WebSocket binary message protocol.

## First-Time Setup

1. **Install Rust**:
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
   source "$HOME/.cargo/env"
   ```

   The project uses a `rust-toolchain.toml` file (`src/rust-toolchain.toml`) — rustup will automatically install the correct version when you run cargo commands from `src/`.

2. **Clone the repository**:
   ```bash
   git clone https://gitlab.com/CAS-eResearch/GWDC/adacs_job_controller.git
   cd adacs_job_controller
   ```

3. **Set up environment**:
   ```bash
   cp .env.template .env
   # Edit .env with your database credentials
   ```

4. **Set up configuration files**:
   ```bash
   cp config/clusters.json.template config/clusters.json
   cp config/access_secrets.json.template config/access_secrets.json
   # Edit with your cluster and application configuration
   ```

## Project Structure

All Rust code is in the `src/` directory. All cargo commands should be run from `src/`:

```bash
cd src
```

## Development

### Building

```bash
# Debug build
cargo build

# Release build
cargo build --release
```

Or use the build script:
```bash
./build_release.sh
```

Output: `target/release/adacs_job_controller`

### Running

```bash
cargo run
```

Make sure `.env` is configured and MySQL is accessible.

### Running Tests

The test suite (500+ tests) uses in-memory SQLite and must run sequentially to avoid race conditions with shared global state.

```bash
# Run all tests
./run_tests.sh

# Run specific test module
./run_tests.sh cluster_tests
./run_tests.sh http_tests
./run_tests.sh db_tests

# Run with verbose output
./run_tests.sh --verbose

# Run specific test function
./run_tests.sh test_handle_job_save -- --nocapture

# Pass any arguments to cargo test
./run_tests.sh -- --test-threads=1 --nocapture
```

### Coverage

```bash
# Install cargo-llvm-cov
cargo install cargo-llvm-cov

# Generate HTML coverage report
./run_tests.sh --coverage

# Generate and open in browser
./run_tests.sh --coverage --open
```

Report location: `target/llvm-cov/html/index.html`

## Docker

### Build and Run

```bash
cd docker
docker compose up --build
```

This builds the Rust binary and starts the application with MySQL.

### Background

```bash
cd docker
docker compose up -d --build
```

### View Logs

```bash
docker compose -f docker/docker-compose.yaml logs -f web
```

### Stop

```bash
cd docker
docker compose down
```

## Architecture

```
HTTP API (:8000)  ──┐
                    ├──→ AppState ──→ ClusterManager ──→ Cluster(s)
WebSocket (:8001) ──┘                                    ├── Priority Queue Scheduler
                                                         ├── Message Resend
                                                         ├── FileDownload/Upload
                                                         └── ClusterDB Dispatch
                                      MySQL ←─────────────┘
```

### Key Modules

| Module | Description |
|--------|-------------|
| `config/` | Settings from env vars, cluster config (`clusters.json`), access secrets (`access_secrets.json`) |
| `protocol/` | Binary message serialization (little-endian, byte-compatible with C++ clients) |
| `cluster/` | Core cluster connection, priority queue scheduler, backpressure, file transfer |
| `http/` | Axum-based REST API — Job CRUD, File download/upload/listing, JWT auth |
| `websocket/` | WebSocket server for binary message protocol with cluster clients |
| `db/` | MySQL connection pool, model structs, ClusterDB message dispatcher |
| `app.rs` | Application state wiring and server startup |

### Binary Protocol

Custom binary format (little-endian) for server↔cluster communication:
```
[source: u64-length-prefix + bytes][messageId: u32][...payload fields...]
```

40+ message types covering job control, file transfer, and database operations.

## API Endpoints

| Method | Path | Description |
|--------|------|-------------|
| POST | `/job/apiv1/job/` | Create and submit a job |
| GET | `/job/apiv1/job/` | Query jobs with filters |
| PATCH | `/job/apiv1/job/` | Cancel a running job |
| DELETE | `/job/apiv1/job/` | Delete a non-running job |
| POST | `/job/apiv1/file/` | Create file download record(s) |
| GET | `/job/apiv1/file/` | Stream file download |
| PUT | `/job/apiv1/file/upload/` | Upload file to cluster |
| PATCH | `/job/apiv1/file/` | List remote files |

All HTTP endpoints require JWT (HS256) authentication via `Authorization: Bearer <token>` header.

## Configuration

The Job Controller requires two JSON configuration files:

1. **Cluster Configuration** (`clusters.json`) - Remote clusters and SSH connection details
2. **Access Secret Configuration** (`access_secrets.json`) - HTTP access and cluster permissions

Paths are specified via environment variables:
- `CLUSTER_CONFIG_FILE` - Path to clusters config (default: `config/clusters.json`)
- `ACCESS_SECRET_CONFIG_FILE` - Path to access secrets (default: `config/access_secrets.json`)

### Cluster Configuration Format

```json
[
  {
    "name": "ozstar",
    "host": "ozstar.swin.edu.au",
    "username": "bilby",
    "path": "/fred/oz988/gwcloud_job_client/",
    "key": "-----BEGIN RSA PRIVATE KEY-----..."
  }
]
```

### Access Secret Configuration Format

```json
[
  {
    "name": "bilbyui",
    "secret": "super_secret",
    "applications": ["gwlab"],
    "clusters": ["ozstar", "cit"]
  }
]
```

### Adding a New Cluster/Application

1. Create SSH user and install job controller client on remote machine
2. Generate RSA SSH key pair:
   ```bash
   ssh-keygen -m PEM -t rsa -b 4096 -C "your_email@example.com"
   ```
3. Add public key to remote SSH user's `~/.ssh/authorized_keys`
4. Edit `config/clusters.json` with cluster name and SSH details
5. Edit `config/access_secrets.json` with application name, JWT secret, and cluster permissions
6. Restart the server:
   ```bash
   docker compose -f docker/docker-compose.yaml restart web
   ```

## Production Deployment

1. Set up `.env`:
   ```bash
   cp .env.template .env
   # Edit with database passwords and configuration
   ```

2. Set up configuration files as described above

3. Start services:
   ```bash
   bash scripts/run.sh
   ```

Configuration files are mounted as read-only volumes at `/app/config/`.

## API Reference

### Job API

#### GET - Query Jobs

Fetch status and/or filter jobs.

**Query Parameters (all optional):**
- `jobIds` - Fetch array of jobs by ID (CSV separated)
- `startTimeGt` - Start time greater than (epoch seconds)
- `startTimeLt` - Start time less than (epoch seconds)
- `endTimeGt` - End time greater than (epoch seconds)
- `endTimeLt` - End time less than (epoch seconds)
- `jobSteps` - Filter by job step state (format: `jobStepId,state` pairs, combined with OR)

**Example:**
```
/job/apiv1/job/?jobIDs=50,51,52&startTimeLt=1589838778&jobSteps=jid0,500,jid1,500
```

**Response:**
```json
[
  {
    "id": 5,
    "user": 32,
    "parameters": "whatever",
    "cluster": "ozstar",
    "bundle": "whatever",
    "history": [
      {
        "jobId": 5,
        "timestamp": 34233,
        "what": "jid0",
        "state": 500
      }
    ]
  }
]
```

#### POST - Create and Submit Job

**Request:**
```json
{
  "cluster": "ozstar",
  "userId": 32,
  "parameters": "whatever",
  "bundle": "whatever"
}
```

**Response:**
```json
{
  "jobId": 56
}
```

#### PATCH - Cancel Job

**Request:**
```json
{
  "jobId": 56
}
```

**Response:**
```json
{
  "cancelled": 56
}
```

#### DELETE - Delete Job

Job must not be in running state.

**Request:**
```json
{
  "jobId": 56
}
```

**Response:**
```json
{
  "deleted": 56
}
```

### File API

#### GET - Download File

Does not require JWT - uses file download ID for authorization.

**Query Parameters:**
- `fileId` - File download ID from POST request
- `forceDownload` - If `true`, triggers attachment download

**Response Headers:**
```
Content-Type: application/octet-stream
Content-Length: <remote file size>
Content-Disposition: attachment; filename="remote file name"
```

#### POST - Create File Download ID(s)

**Request (single path):**
```json
{
  "jobId": 56,
  "path": "whatever"
}
```

**Request (multiple paths):**
```json
{
  "jobId": 56,
  "paths": ["path1", "path2"]
}
```

**Response (single):**
```json
{
  "fileId": "some-uuid"
}
```

**Response (multiple):**
```json
{
  "fileIds": ["uuid1", "uuid2"]
}
```

#### PATCH - List Remote Files

**Request:**
```json
{
  "jobId": 56,
  "recursive": true,
  "path": "/my/path/"
}
```

**Response:**
```json
{
  "files": [
    {
      "path": "/file/path",
      "isDir": false,
      "fileSize": 345652,
      "permissions": null
    }
  ]
}
```

## Troubleshooting

### Database connection errors

Ensure MySQL is running and credentials in `.env` are correct:
```bash
# Check MySQL is accessible
mysql -h localhost -u gwcloud -p
```

### Clean build
```bash
cargo clean
cargo build
```

### Test failures

Tests require sequential execution (`--test-threads=1`) due to shared global state. The `run_tests.sh` script handles this automatically.

## Requirements

- **Build**: Rust stable (pinned in `src/rust-toolchain.toml`, rustup will auto-install)
- **Runtime**: MySQL database, configured clusters and access secrets
- **Test**: None (tests use in-memory SQLite)

## CI/CD

The project uses GitLab CI with three stages:

1. **lint**: rustfmt and clippy checks
2. **test**: Test execution with coverage reporting
3. **release**: Build optimized release binary

Run locally with:
```bash
gitlab-ci-local
```
