#![cfg(feature = "sqlite")]

use serde_json::json;
use std::{
    path::PathBuf,
    sync::{
        Arc, Barrier,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};
use vtabs_store::{ErrorCode, Key, Operation, Request, Scope, sqlite};

struct Database(PathBuf);
static NEXT_DATABASE: AtomicU64 = AtomicU64::new(0);
impl Database {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let sequence = NEXT_DATABASE.fetch_add(1, Ordering::Relaxed);
        Self(std::env::temp_dir().join(format!(
            "vtabs-store-{}-{nonce}-{sequence}.sqlite",
            std::process::id()
        )))
    }
}
impl Drop for Database {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
fn key(field: &str) -> Key {
    Key {
        scope: Scope::profile("default"),
        entity: "space:work".into(),
        field: field.into(),
    }
}
fn put(field: &str, value: &str, revision: Option<u64>) -> Operation {
    Operation::Put {
        key: key(field),
        value: json!(value),
        expected_revision: revision,
    }
}

#[test]
fn fresh_database_migrates_once_and_preserves_tombstone_revisions() {
    let db = Database::new();
    let mut connection = sqlite::open(&db.0).unwrap();
    let created = sqlite::execute(
        &mut connection,
        &Request::new(1, vec![put("name", "Work", Some(0))]),
    )
    .unwrap();
    assert_eq!(created.revision, 1);
    drop(connection);
    let mut connection = sqlite::open(&db.0).unwrap();
    let deleted = sqlite::execute(
        &mut connection,
        &Request::new(
            2,
            vec![Operation::Delete {
                key: key("name"),
                expected_revision: Some(1),
            }],
        ),
    )
    .unwrap();
    assert_eq!(deleted.records[0].revision, 2);
    assert_eq!(deleted.records[0].value, None);
    let stale = sqlite::execute(
        &mut connection,
        &Request::new(3, vec![put("name", "Stale", Some(0))]),
    )
    .unwrap_err();
    assert_eq!(stale.code, ErrorCode::Conflict);
    assert_eq!(stale.actual_revision, Some(2));
}

#[test]
fn conflict_rolls_back_the_whole_batch() {
    let db = Database::new();
    let mut connection = sqlite::open(&db.0).unwrap();
    sqlite::execute(
        &mut connection,
        &Request::new(1, vec![put("name", "Work", Some(0))]),
    )
    .unwrap();
    let result = sqlite::execute(
        &mut connection,
        &Request::new(
            2,
            vec![put("icon", "W", Some(0)), put("name", "Wrong", Some(0))],
        ),
    );
    assert_eq!(result.unwrap_err().code, ErrorCode::Conflict);
    let read = sqlite::execute(
        &mut connection,
        &Request::new(
            3,
            vec![Operation::Read {
                scope: Scope::profile("default"),
            }],
        ),
    )
    .unwrap();
    assert_eq!(read.revision, 1);
    assert_eq!(read.records.len(), 1);
    assert_eq!(read.records[0].value, Some(json!("Work")));
}

#[test]
fn concurrent_windows_merge_independent_fields_and_reject_same_field_staleness() {
    let db = Database::new();
    drop(sqlite::open(&db.0).unwrap());
    let barrier = Arc::new(Barrier::new(2));
    let handles: Vec<_> = ["name", "icon"]
        .into_iter()
        .map(|field| {
            let path = db.0.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let mut connection = sqlite::open(&path).unwrap();
                barrier.wait();
                sqlite::execute(
                    &mut connection,
                    &Request::new(1, vec![put(field, field, Some(0))]),
                )
                .unwrap()
            })
        })
        .collect();
    for handle in handles {
        handle.join().unwrap();
    }
    let mut connection = sqlite::open(&db.0).unwrap();
    let read = sqlite::execute(
        &mut connection,
        &Request::new(
            4,
            vec![Operation::Read {
                scope: Scope::profile("default"),
            }],
        ),
    )
    .unwrap();
    assert_eq!(read.records.len(), 2);
    assert_eq!(read.revision, 2);
    let stale = sqlite::execute(
        &mut connection,
        &Request::new(5, vec![put("name", "stale", Some(0))]),
    )
    .unwrap_err();
    assert_eq!(stale.code, ErrorCode::Conflict);
}

#[test]
fn private_writes_and_unverified_session_keys_are_rejected() {
    let db = Database::new();
    let mut connection = sqlite::open(&db.0).unwrap();
    let mut private = Request::new(1, vec![put("name", "private", None)]);
    private.private = true;
    assert_eq!(
        sqlite::execute(&mut connection, &private).unwrap_err().code,
        ErrorCode::PrivateWrite
    );
    let invalid = Request::new(
        2,
        vec![Operation::Read {
            scope: Scope::Session {
                profile: "default".into(),
                incarnation: String::new(),
            },
        }],
    );
    assert_eq!(
        sqlite::execute(&mut connection, &invalid).unwrap_err().code,
        ErrorCode::InvalidRequest
    );
    let read = sqlite::execute(
        &mut connection,
        &Request::new(
            3,
            vec![Operation::Read {
                scope: Scope::profile("default"),
            }],
        ),
    )
    .unwrap();
    assert_eq!(read.revision, 0);
    assert!(read.records.is_empty());
}

#[test]
fn session_incarnations_never_share_reused_tab_ids() {
    let db = Database::new();
    let mut connection = sqlite::open(&db.0).unwrap();
    let first = Scope::Session {
        profile: "default".into(),
        incarnation: "connection-a".into(),
    };
    let second = Scope::Session {
        profile: "default".into(),
        incarnation: "connection-b".into(),
    };
    sqlite::execute(
        &mut connection,
        &Request::new(
            1,
            vec![Operation::Put {
                key: Key {
                    scope: first,
                    entity: "tab:1".into(),
                    field: "space".into(),
                },
                value: json!("work"),
                expected_revision: Some(0),
            }],
        ),
    )
    .unwrap();
    let read = sqlite::execute(
        &mut connection,
        &Request::new(2, vec![Operation::Read { scope: second }]),
    )
    .unwrap();
    assert!(read.records.is_empty());
}

#[test]
fn newer_schema_is_rejected_without_changes() {
    let db = Database::new();
    let connection = sqlite::open(&db.0).unwrap();
    connection.pragma_update(None, "user_version", 99).unwrap();
    drop(connection);
    assert_eq!(
        sqlite::open(&db.0).unwrap_err().code,
        ErrorCode::NewerSchema
    );
    let connection = rusqlite::Connection::open(&db.0).unwrap();
    assert_eq!(
        connection
            .pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))
            .unwrap(),
        99
    );
}

#[test]
fn limits_fail_before_any_operation_is_committed() {
    let db = Database::new();
    let mut connection = sqlite::open(&db.0).unwrap();
    let request = Request::new(
        1,
        vec![
            put("name", "valid", None),
            put("icon", &"x".repeat(vtabs_store::MAX_VALUE_BYTES), None),
        ],
    );
    assert_eq!(
        sqlite::execute(&mut connection, &request).unwrap_err().code,
        ErrorCode::Limit
    );
    let read = sqlite::execute(
        &mut connection,
        &Request::new(
            2,
            vec![Operation::Read {
                scope: Scope::profile("default"),
            }],
        ),
    )
    .unwrap();
    assert!(read.records.is_empty());
}

#[test]
fn json_null_is_distinct_from_a_tombstone_over_the_wire() {
    let null = vtabs_store::Record {
        key: key("accent"),
        value: Some(serde_json::Value::Null),
        revision: 1,
    };
    let tombstone = vtabs_store::Record {
        key: key("accent"),
        value: None,
        revision: 2,
    };
    for record in [null, tombstone] {
        let encoded = serde_json::to_vec(&record).unwrap();
        let decoded: vtabs_store::Record = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded, record);
    }
}

#[test]
fn oversized_read_rolls_back_earlier_writes_in_the_batch() {
    let db = Database::new();
    let mut connection = sqlite::open(&db.0).unwrap();
    let value = "x".repeat(vtabs_store::MAX_VALUE_BYTES - 2);
    for batch in 0..5 {
        let writes = (0..16)
            .map(|index| put(&format!("large-{}", batch * 16 + index), &value, Some(0)))
            .collect();
        sqlite::execute(&mut connection, &Request::new(batch, writes)).unwrap();
    }
    let request = Request::new(
        10,
        vec![
            put("must-rollback", "temporary", Some(0)),
            Operation::Read {
                scope: Scope::profile("default"),
            },
        ],
    );
    assert_eq!(
        sqlite::execute(&mut connection, &request).unwrap_err().code,
        ErrorCode::Limit
    );
    let revision: i64 = connection
        .query_row("SELECT revision FROM metadata WHERE id=1", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(revision, 5);
    let absent: i64 = connection
        .query_row(
            "SELECT count(*) FROM fields WHERE field='must-rollback'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(absent, 0);
}
