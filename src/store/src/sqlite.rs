use crate::*;
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use std::{io, path::Path, time::Duration};

const SCHEMA_VERSION: u32 = 1;

fn database(error: rusqlite::Error) -> StoreError {
    StoreError::new(ErrorCode::Database, error.to_string())
}

fn row_revision(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<u64> {
    u64::try_from(row.get::<_, i64>(index)?).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            index,
            rusqlite::types::Type::Integer,
            Box::new(error),
        )
    })
}

fn valid_component(value: &str) -> Result<(), StoreError> {
    if value.is_empty() || value.len() > MAX_KEY_BYTES || value.contains('\0') {
        return Err(StoreError::new(
            ErrorCode::InvalidRequest,
            "invalid storage key",
        ));
    }
    Ok(())
}

fn validate_scope(scope: &Scope) -> Result<(), StoreError> {
    match scope {
        Scope::Profile { profile } => valid_component(profile)?,
        Scope::Session {
            profile,
            incarnation,
        } => {
            valid_component(profile)?;
            valid_component(incarnation)?;
        }
    }
    Ok(())
}

fn scope_id(scope: &Scope) -> Result<String, StoreError> {
    serde_json::to_string(scope)
        .map_err(|error| StoreError::new(ErrorCode::InvalidRequest, error.to_string()))
}

#[derive(Default)]
struct JsonSize(usize);

impl io::Write for JsonSize {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0 += bytes.len();
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn serialized_size(value: &impl Serialize) -> serde_json::Result<usize> {
    let mut size = JsonSize::default();
    serde_json::to_writer(&mut size, value)?;
    Ok(size.0)
}

fn validate(request: &Request) -> Result<(), StoreError> {
    if request.version != PROTOCOL_VERSION {
        return Err(StoreError::new(
            ErrorCode::InvalidRequest,
            "unsupported protocol version",
        ));
    }
    if request.operations.len() > MAX_OPERATIONS {
        return Err(StoreError::new(ErrorCode::Limit, "too many operations"));
    }
    for operation in &request.operations {
        let key = match operation {
            Operation::Read { scope } => {
                validate_scope(scope)?;
                continue;
            }
            Operation::Put { key, value, .. } => {
                if serialized_size(value).map_err(|error| {
                    StoreError::new(ErrorCode::InvalidRequest, error.to_string())
                })? > MAX_VALUE_BYTES
                {
                    return Err(StoreError::new(ErrorCode::Limit, "value too large"));
                }
                key
            }
            Operation::Delete { key, .. } => key,
        };
        if request.private {
            return Err(StoreError::new(
                ErrorCode::PrivateWrite,
                "private writes are disabled",
            ));
        }
        validate_scope(&key.scope)?;
        valid_component(&key.entity)?;
        valid_component(&key.field)?;
    }
    Ok(())
}

pub fn open(path: &Path) -> Result<Connection, StoreError> {
    let mut connection = Connection::open(path).map_err(database)?;
    connection
        .busy_timeout(Duration::from_millis(1500))
        .map_err(database)?;
    connection
        .pragma_update(None, "foreign_keys", true)
        .map_err(database)?;
    let version: u32 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(database)?;
    match version {
        SCHEMA_VERSION => return Ok(connection),
        0 => {}
        _ => {
            return Err(StoreError::new(
                ErrorCode::NewerSchema,
                "database schema is newer than this helper",
            ));
        }
    }
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(database)?;
    let version: u32 = transaction
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(database)?;
    match version {
        0 => {
            transaction.execute_batch(
                "CREATE TABLE metadata (id INTEGER PRIMARY KEY CHECK(id = 1), revision INTEGER NOT NULL);
                 INSERT INTO metadata VALUES (1, 0);
                 CREATE TABLE fields (
                    scope TEXT NOT NULL,
                    entity TEXT NOT NULL,
                    field TEXT NOT NULL,
                    value TEXT,
                    revision INTEGER NOT NULL,
                    PRIMARY KEY (scope, entity, field)
                 ) WITHOUT ROWID;
                 PRAGMA user_version = 1;"
            ).map_err(database)?;
        }
        SCHEMA_VERSION => {}
        _ => {
            return Err(StoreError::new(
                ErrorCode::NewerSchema,
                "database schema is newer than this helper",
            ));
        }
    }
    transaction.commit().map_err(database)?;
    Ok(connection)
}

fn write(
    transaction: &Transaction<'_>,
    key: &Key,
    value: Option<&Value>,
    expected_revision: Option<u64>,
    revision: u64,
) -> Result<Record, StoreError> {
    let scope = scope_id(&key.scope)?;
    if let Some(expected_revision) = expected_revision {
        let actual_revision: u64 = transaction
            .prepare_cached("SELECT revision FROM fields WHERE scope=?1 AND entity=?2 AND field=?3")
            .map_err(database)?
            .query_row(params![scope, key.entity, key.field], |row| {
                row_revision(row, 0)
            })
            .optional()
            .map_err(database)?
            .unwrap_or(0);
        if expected_revision != actual_revision {
            return Err(StoreError {
                code: ErrorCode::Conflict,
                message: "field revision changed".into(),
                key: Some(Box::new(key.clone())),
                actual_revision: Some(actual_revision),
            });
        }
    }
    let serialized = value
        .map(serde_json::to_string)
        .transpose()
        .map_err(|error| StoreError::new(ErrorCode::InvalidRequest, error.to_string()))?;
    transaction.prepare_cached(
        "INSERT INTO fields (scope,entity,field,value,revision) VALUES (?1,?2,?3,?4,?5)
         ON CONFLICT(scope,entity,field) DO UPDATE SET value=excluded.value,revision=excluded.revision"
    ).map_err(database)?.execute(
        params![scope, key.entity, key.field, serialized, revision as i64],
    ).map_err(database)?;
    Ok(Record {
        key: key.clone(),
        value: value.cloned(),
        revision,
    })
}

fn append_record(
    records: &mut Vec<Record>,
    bytes: &mut usize,
    record: Record,
) -> Result<(), StoreError> {
    let record_bytes = serialized_size(&record)
        .map_err(|error| StoreError::new(ErrorCode::Database, error.to_string()))?
        + usize::from(!records.is_empty());
    if records.len() >= MAX_RECORDS || *bytes + record_bytes > MAX_RESPONSE_BYTES {
        return Err(StoreError::new(ErrorCode::Limit, "response too large"));
    }
    *bytes += record_bytes;
    records.push(record);
    Ok(())
}

pub fn execute(connection: &mut Connection, request: &Request) -> Result<Response, StoreError> {
    validate(request)?;
    let has_writes = request
        .operations
        .iter()
        .any(|operation| !matches!(operation, Operation::Read { .. }));
    let behavior = if has_writes {
        TransactionBehavior::Immediate
    } else {
        TransactionBehavior::Deferred
    };
    let transaction = connection
        .transaction_with_behavior(behavior)
        .map_err(database)?;
    let previous_revision: u64 = transaction
        .prepare_cached("SELECT revision FROM metadata WHERE id=1")
        .map_err(database)?
        .query_row([], |row| row_revision(row, 0))
        .map_err(database)?;
    let revision = if has_writes {
        previous_revision
            .checked_add(1)
            .filter(|value| *value <= i64::MAX as u64)
            .ok_or_else(|| StoreError::new(ErrorCode::Limit, "revision exhausted"))?
    } else {
        previous_revision
    };
    let mut response = Response {
        version: PROTOCOL_VERSION,
        request_id: request.request_id,
        revision,
        records: Vec::new(),
        error: None,
    };
    let mut record_bytes = serialized_size(&response)
        .map_err(|error| StoreError::new(ErrorCode::Database, error.to_string()))?;
    for operation in &request.operations {
        match operation {
            Operation::Read { scope } => {
                let scope_text = scope_id(scope)?;
                let mut statement = transaction.prepare_cached("SELECT entity,field,value,revision FROM fields WHERE scope=?1 ORDER BY entity,field LIMIT ?2").map_err(database)?;
                let rows = statement
                    .query_map(params![scope_text, (MAX_RECORDS + 1) as i64], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, Option<String>>(2)?,
                            row_revision(row, 3)?,
                        ))
                    })
                    .map_err(database)?;
                for row in rows {
                    let (entity, field, serialized, revision) = row.map_err(database)?;
                    let value = serialized
                        .map(|text| serde_json::from_str(&text))
                        .transpose()
                        .map_err(|error| StoreError::new(ErrorCode::Database, error.to_string()))?;
                    append_record(
                        &mut response.records,
                        &mut record_bytes,
                        Record {
                            key: Key {
                                scope: scope.clone(),
                                entity,
                                field,
                            },
                            value,
                            revision,
                        },
                    )?;
                }
            }
            Operation::Put {
                key,
                value,
                expected_revision,
            } => append_record(
                &mut response.records,
                &mut record_bytes,
                write(&transaction, key, Some(value), *expected_revision, revision)?,
            )?,
            Operation::Delete {
                key,
                expected_revision,
            } => append_record(
                &mut response.records,
                &mut record_bytes,
                write(&transaction, key, None, *expected_revision, revision)?,
            )?,
        }
    }
    if has_writes {
        transaction
            .prepare_cached("UPDATE metadata SET revision=?1 WHERE id=1")
            .map_err(database)?
            .execute([revision as i64])
            .map_err(database)?;
    }
    transaction.commit().map_err(database)?;
    Ok(response)
}
