//! Integration tests for binary protocol compatibility.
//!
//! Verifies that the binary message format produces exact, deterministic byte sequences
//! and that messages survive full serialization round-trips without data loss.

use adacs_job_controller::db::models::{BundleJob, ClusterJob, ClusterJobStatus};
use adacs_job_controller::protocol::constants::*;
use adacs_job_controller::protocol::message::Message;
use adacs_job_controller::protocol::types::{FileInfo, Priority};

// ---------------------------------------------------------------------------
// Exact byte-sequence tests
// ---------------------------------------------------------------------------

/// Verifies that a `SUBMIT_JOB` message serializes and deserializes all fields without data loss.
///
/// # Setup
/// A `SUBMIT_JOB` message with source `"job_1_cluster_a"`, `job_id=42`, bundle `"my_bundle"`,
/// and params `"--param1 value1"` is constructed.
///
/// # Act
/// The message is round-tripped through `data()` / `from_bytes`.
///
/// # Assert
/// All fields — source, message ID, `job_id`, bundle, and params — are correctly recovered.
#[test]
fn test_submit_job_message_exact_bytes() {
    // Build a SUBMIT_JOB message matching the expected wire format:
    //   header: source(string) + msg_id(u32)
    //   payload: job_id(u32) + bundle(string) + params(string)
    let source = "job_1_cluster_a";
    let mut msg = Message::new(SUBMIT_JOB, Priority::Medium, source);
    msg.push_uint(42); // job_id
    msg.push_string("my_bundle");
    msg.push_string("--param1 value1");

    let data = msg.data().to_vec();
    // Parse it back and verify every field
    let mut parsed = Message::from_bytes(data);
    assert_eq!(parsed.source(), source);
    assert_eq!(parsed.id(), SUBMIT_JOB);
    assert_eq!(parsed.pop_uint(), 42);
    assert_eq!(parsed.pop_string(), "my_bundle");
    assert_eq!(parsed.pop_string(), "--param1 value1");
}

/// Verifies the exact byte layout of the `SUBMIT_JOB` message header.
///
/// # Setup
/// A `SUBMIT_JOB` message with source `"job_1_cluster_a"` is constructed.
///
/// # Act
/// The raw byte slice is inspected at known offsets.
///
/// # Assert
/// Bytes 0–7 are the little-endian length of the source string; bytes 8–22 are the source UTF-8
/// bytes; the following 4 bytes are the little-endian `SUBMIT_JOB` message ID constant.
#[test]
fn test_submit_job_header_byte_layout() {
    let source = "job_1_cluster_a";
    let msg = Message::new(SUBMIT_JOB, Priority::Medium, source);
    let data = msg.data();

    // Byte layout of source string: 8-byte LE length prefix + UTF-8 bytes
    let expected_source_len = source.len() as u64;
    assert_eq!(
        &data[0..8],
        &expected_source_len.to_le_bytes(),
        "source length prefix"
    );
    assert_eq!(
        &data[8..8 + source.len()],
        source.as_bytes(),
        "source string bytes"
    );

    // msg_id follows immediately (little-endian u32)
    let id_offset = 8 + source.len();
    assert_eq!(
        &data[id_offset..id_offset + 4],
        &SUBMIT_JOB.to_le_bytes(),
        "message id bytes"
    );
}

/// Verifies that a `DB_RESPONSE` message carrying a request ID, count, and result ulong serializes correctly.
///
/// # Setup
/// A `DB_RESPONSE` message is built with `db_request_id=12345`, count `1`, and result ID `99`.
///
/// # Act
/// The message is round-tripped through `into_data` / `from_bytes`.
///
/// # Assert
/// The parsed source, message ID, `db_request_id`, count, and result ulong all match.
#[test]
fn test_db_response_message_format() {
    let db_request_id: u32 = 12345;
    let mut msg = Message::new(DB_RESPONSE, Priority::Highest, SYSTEM_SOURCE);
    msg.push_uint(db_request_id);
    msg.push_uint(1); // count
    msg.push_ulong(99); // some result id

    let data = msg.into_data();
    let mut parsed = Message::from_bytes(data);

    assert_eq!(parsed.source(), SYSTEM_SOURCE);
    assert_eq!(parsed.id(), DB_RESPONSE);
    assert_eq!(parsed.pop_uint(), db_request_id);
    assert_eq!(parsed.pop_uint(), 1);
    assert_eq!(parsed.pop_ulong(), 99);
}

/// Verifies that a `FILE_LIST` response message with multiple entries round-trips without data loss.
///
/// # Setup
/// Three `FileInfo` entries (two files and one directory) are defined.
/// A `FILE_LIST` message is built containing a UUID, entry count, and the file metadata.
///
/// # Act
/// The message is round-tripped through `into_data` / `from_bytes`.
///
/// # Assert
/// The parsed UUID, entry count, and all `file_name`, `is_directory`, and `file_size`
/// values for each entry match the originals.
#[test]
fn test_file_list_response_with_multiple_entries() {
    // Build a FILE_LIST response message the same way the cluster does:
    //   uuid(string) + num_files(u32) + [file_name(string) + is_dir(bool) + file_size(u64)]*
    let uuid = "abcd-1234-efgh-5678";
    let files = vec![
        FileInfo {
            file_name: "/project/file1.txt".to_string(),
            file_size: 1024,
            permissions: 0o644,
            is_directory: false,
        },
        FileInfo {
            file_name: "/project/subdir".to_string(),
            file_size: 0,
            permissions: 0o755,
            is_directory: true,
        },
        FileInfo {
            file_name: "/project/subdir/file2.dat".to_string(),
            file_size: 999_999,
            permissions: 0o644,
            is_directory: false,
        },
    ];

    let mut msg = Message::new(FILE_LIST, Priority::Medium, "test_cluster");
    msg.push_string(uuid);
    msg.push_uint(files.len().try_into().unwrap());
    for f in &files {
        msg.push_string(&f.file_name);
        msg.push_bool(f.is_directory);
        msg.push_ulong(f.file_size);
    }

    // Round-trip
    let mut parsed = Message::from_bytes(msg.into_data());
    assert_eq!(parsed.source(), "test_cluster");
    assert_eq!(parsed.id(), FILE_LIST);
    assert_eq!(parsed.pop_string(), uuid);

    let num = parsed.pop_uint();
    assert_eq!(num, 3);

    for expected in &files {
        assert_eq!(parsed.pop_string(), expected.file_name);
        assert_eq!(parsed.pop_bool(), expected.is_directory);
        assert_eq!(parsed.pop_ulong(), expected.file_size);
    }
}

// ---------------------------------------------------------------------------
// Full round-trip tests for DB model serialization
// ---------------------------------------------------------------------------

/// Verifies that a `ClusterJob` struct survives a full serialization round-trip inside a `DB_JOB_SAVE` message.
///
/// # Setup
/// A `ClusterJob` with all populated fields (including `job_id=42`, `bundle_hash`, and `working_directory`)
/// is wrapped in a `DB_JOB_SAVE` message with `db_request_id=777`.
///
/// # Act
/// The message is round-tripped through `into_data` / `from_bytes`.
///
/// # Assert
/// Every field of the deserialized `ClusterJob` matches the original.
#[test]
fn test_cluster_job_full_roundtrip_via_message() {
    let original = ClusterJob {
        id: 0, // new record (insert)
        job_id: 42,
        scheduler_id: 1001,
        submitting: true,
        submitting_count: 2,
        bundle_hash: "sha256:abc123".to_string(),
        working_directory: "/scratch/user/job42".to_string(),
        running: false,
        deleting: false,
        deleted: false,
        cluster: "test_cluster".to_string(),
    };

    // Wrap in a DB_JOB_SAVE message like the cluster client would
    let mut msg = Message::new(DB_JOB_SAVE, Priority::Highest, SYSTEM_SOURCE);
    let db_request_id: u32 = 777;
    msg.push_uint(db_request_id);
    original.to_message(&mut msg);

    // Parse back
    let mut parsed = Message::from_bytes(msg.into_data());
    assert_eq!(parsed.source(), SYSTEM_SOURCE);
    assert_eq!(parsed.id(), DB_JOB_SAVE);
    assert_eq!(parsed.pop_uint(), db_request_id);

    let restored = ClusterJob::from_message(&mut parsed);
    assert_eq!(restored.id, original.id);
    assert_eq!(restored.job_id, original.job_id);
    assert_eq!(restored.scheduler_id, original.scheduler_id);
    assert_eq!(restored.submitting, original.submitting);
    assert_eq!(restored.submitting_count, original.submitting_count);
    assert_eq!(restored.bundle_hash, original.bundle_hash);
    assert_eq!(restored.working_directory, original.working_directory);
    assert_eq!(restored.running, original.running);
    assert_eq!(restored.deleting, original.deleting);
    assert_eq!(restored.deleted, original.deleted);
}

/// Verifies that a `ClusterJobStatus` struct survives a full serialization round-trip inside a `DB_JOBSTATUS_SAVE` message.
///
/// # Setup
/// A `ClusterJobStatus` with `job_id=100`, `what="scheduler_id"`, and `state=500`
/// is wrapped in a `DB_JOBSTATUS_SAVE` message with `db_request_id=888`.
///
/// # Act
/// The message is round-tripped through `into_data` / `from_bytes`.
///
/// # Assert
/// All fields — `id`, `job_id`, `what`, and `state` — match the original.
#[test]
fn test_cluster_job_status_full_roundtrip_via_message() {
    let original = ClusterJobStatus {
        id: 0,
        job_id: 100,
        what: "scheduler_id".to_string(),
        state: 500,
    };

    let mut msg = Message::new(DB_JOBSTATUS_SAVE, Priority::Highest, SYSTEM_SOURCE);
    let db_request_id: u32 = 888;
    msg.push_uint(db_request_id);
    original.to_message(&mut msg);

    let mut parsed = Message::from_bytes(msg.into_data());
    assert_eq!(parsed.id(), DB_JOBSTATUS_SAVE);
    assert_eq!(parsed.pop_uint(), db_request_id);

    let restored = ClusterJobStatus::from_message(&mut parsed);
    assert_eq!(restored.id, original.id);
    assert_eq!(restored.job_id, original.job_id);
    assert_eq!(restored.what, original.what);
    assert_eq!(restored.state, original.state);
}

/// Verifies that a `BundleJob` struct survives a full serialization round-trip inside a `DB_BUNDLE_CREATE_OR_UPDATE_JOB` message.
///
/// # Setup
/// A `BundleJob` with a JSON script content, cluster `"ozstar"`, and a bundle hash
/// is wrapped in a `DB_BUNDLE_CREATE_OR_UPDATE_JOB` message with `db_request_id=999`.
///
/// # Act
/// The message is round-tripped through `into_data` / `from_bytes`.
///
/// # Assert
/// The deserialized `BundleJob` `id` and `content` match the original.
#[test]
fn test_bundle_job_full_roundtrip_via_message() {
    let original = BundleJob {
        id: 0,
        content: r##"{"script":"#!/bin/bash\necho hello"}"##.to_string(),
        cluster: "ozstar".to_string(),
        bundle_hash: "hash_xyz".to_string(),
    };

    let mut msg = Message::new(
        DB_BUNDLE_CREATE_OR_UPDATE_JOB,
        Priority::Highest,
        SYSTEM_SOURCE,
    );
    let db_request_id: u32 = 999;
    msg.push_uint(db_request_id);
    original.to_message(&mut msg);

    let mut parsed = Message::from_bytes(msg.into_data());
    assert_eq!(parsed.id(), DB_BUNDLE_CREATE_OR_UPDATE_JOB);
    assert_eq!(parsed.pop_uint(), db_request_id);

    let restored = BundleJob::from_message(&mut parsed);
    assert_eq!(restored.id, original.id);
    assert_eq!(restored.content, original.content);
}

// ---------------------------------------------------------------------------
// Cross-module: message ID constants are consistent
// ---------------------------------------------------------------------------

/// Verifies that all DB-related message ID constants have unique values.
///
/// # Setup
/// An array of all 13 DB message ID constants is assembled.
///
/// # Act
/// Each ID is inserted into a `HashSet`.
///
/// # Assert
/// No duplicate IDs are found; the assertion fails with the duplicate value if any collision occurs.
#[test]
fn test_all_db_message_ids_are_unique() {
    let ids = [
        DB_JOB_GET_BY_JOB_ID,
        DB_JOB_GET_BY_ID,
        DB_JOB_GET_RUNNING_JOBS,
        DB_JOB_DELETE,
        DB_JOB_SAVE,
        DB_JOBSTATUS_GET_BY_JOB_ID_AND_WHAT,
        DB_JOBSTATUS_GET_BY_JOB_ID,
        DB_JOBSTATUS_DELETE_BY_ID_LIST,
        DB_JOBSTATUS_SAVE,
        DB_RESPONSE,
        DB_BUNDLE_CREATE_OR_UPDATE_JOB,
        DB_BUNDLE_GET_JOB_BY_ID,
        DB_BUNDLE_DELETE_JOB,
    ];
    let mut seen = std::collections::HashSet::new();
    for id in &ids {
        assert!(seen.insert(*id), "Duplicate message ID: {id}");
    }
}

/// Verifies that all file transfer message ID constants have unique values.
///
/// # Setup
/// An array of all 12 file-related message ID constants is assembled.
///
/// # Act
/// Each ID is inserted into a `HashSet`.
///
/// # Assert
/// No duplicate IDs are found; the assertion fails with the duplicate value if any collision occurs.
#[test]
fn test_all_file_message_ids_are_unique() {
    let ids = [
        DOWNLOAD_FILE,
        FILE_DETAILS,
        FILE_ERROR,
        FILE_CHUNK,
        PAUSE_FILE_CHUNK_STREAM,
        RESUME_FILE_CHUNK_STREAM,
        FILE_LIST,
        FILE_LIST_ERROR,
        UPLOAD_FILE,
        FILE_UPLOAD_CHUNK,
        FILE_UPLOAD_ERROR,
        FILE_UPLOAD_COMPLETE,
    ];
    let mut seen = std::collections::HashSet::new();
    for id in &ids {
        assert!(seen.insert(*id), "Duplicate file message ID: {id}");
    }
}

// ---------------------------------------------------------------------------
// Stress: large payload round-trip
// ---------------------------------------------------------------------------

/// Verifies that very large string and byte payloads survive a round-trip without data loss or truncation.
///
/// # Setup
/// A 1 MB string and a 500 KB byte slice (cycling 0–255) are prepared.
/// A `SUBMIT_JOB` message with source `"stress_test"` is built containing both payloads plus a sentinel `u32`.
///
/// # Act
/// The message is round-tripped through `into_data` / `from_bytes`.
///
/// # Assert
/// The source, ID, large string, large bytes, and sentinel `u32` are all correctly recovered.
#[test]
fn test_large_payload_roundtrip() {
    let big_string = "X".repeat(1_000_000);
    let big_bytes: Vec<u8> = (0..=255).cycle().take(500_000).collect();

    let mut msg = Message::new(SUBMIT_JOB, Priority::Medium, "stress_test");
    msg.push_string(&big_string);
    msg.push_bytes(&big_bytes);
    msg.push_uint(0xDEAD_BEEF);

    let mut parsed = Message::from_bytes(msg.into_data());
    assert_eq!(parsed.source(), "stress_test");
    assert_eq!(parsed.id(), SUBMIT_JOB);
    assert_eq!(parsed.pop_string(), big_string);
    assert_eq!(parsed.pop_bytes(), big_bytes);
    assert_eq!(parsed.pop_uint(), 0xDEAD_BEEF);
}

/// Verifies that a message with an empty source string serializes and parses correctly.
///
/// # Setup
/// A `SERVER_READY` message with an empty source `""` and a single `bool` payload is constructed.
///
/// # Act
/// The message is round-tripped through `into_data` / `from_bytes`.
///
/// # Assert
/// The parsed source is `""`, the ID is `SERVER_READY`, and the bool payload is `true`.
#[test]
fn test_empty_source_message_roundtrip() {
    let mut msg = Message::new(SERVER_READY, Priority::Highest, "");
    msg.push_bool(true);

    let mut parsed = Message::from_bytes(msg.into_data());
    assert_eq!(parsed.source(), "");
    assert_eq!(parsed.id(), SERVER_READY);
    assert!(parsed.pop_bool());
}

/// Verifies that a message with a multi-byte Unicode source string serializes and parses correctly.
///
/// # Setup
/// An `UPDATE_JOB` message with a Japanese Unicode source `"クラスタ_α"` and a Unicode string payload is constructed.
///
/// # Act
/// The message is round-tripped through `into_data` / `from_bytes`.
///
/// # Assert
/// The parsed source and string payload are the original Unicode strings.
#[test]
fn test_unicode_source_roundtrip() {
    let source = "クラスタ_α";
    let mut msg = Message::new(UPDATE_JOB, Priority::Medium, source);
    msg.push_string("日本語テスト");

    let mut parsed = Message::from_bytes(msg.into_data());
    assert_eq!(parsed.source(), source);
    assert_eq!(parsed.pop_string(), "日本語テスト");
}
