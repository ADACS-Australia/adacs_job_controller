# ADACS Job Controller (Rust)

A Rust port of the ADACS Job Controller — a server that manages distributed job submissions to HPC/compute clusters via an HTTP REST API and WebSocket binary message protocol.

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

### Key Components

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

## Building

```bash
cargo build --release
```

## Testing

```bash
cargo test
```

## Configuration

Copy `.env.template` to `.env` and configure:
- Database credentials (`MYSQL_*`)
- Buffer sizes (`MAX_FILE_BUFFER_SIZE`, etc.)
- Config file paths (`CLUSTER_CONFIG_FILE`, `ACCESS_SECRET_CONFIG_FILE`)

Create `config/clusters.json` and `config/access_secrets.json` from templates in the parent `config/` directory.

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

## Dependencies

- **tokio** — async runtime
- **axum** — HTTP/WebSocket server
- **sqlx** — async MySQL driver
- **dashmap** — concurrent hash map
- **jsonwebtoken** — JWT authentication
- **serde** / **serde_json** — serialization
- **tracing** — structured logging
- **mockall** — trait-based mocking for tests
