use crate::*;
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use std::{path::Path, time::Duration};

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

fn scope_id(scope: &Scope) -> Result<String, StoreError> {
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
    serde_json::to_string(scope)
        .map_err(|error| StoreError::new(ErrorCode::InvalidRequest, error.to_string()))
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
                scope_id(scope)?;
                continue;
            }
            Operation::Put { key, value, .. } => {
                if serde_json::to_vec(value)
                    .map_err(|error| StoreError::new(ErrorCode::InvalidRequest, error.to_string()))?
                    .len()
                    > MAX_VALUE_BYTES
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
        scope_id(&key.scope)?;
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
    let actual_revision: u64 = transaction
        .query_row(
            "SELECT revision FROM fields WHERE scope=?1 AND entity=?2 AND field=?3",
            params![scope, key.entity, key.field],
            |row| row_revision(row, 0),
        )
        .optional()
        .map_err(database)?
        .unwrap_or(0);
    if expected_revision.is_some_and(|expected| expected != actual_revision) {
        return Err(StoreError {
            code: ErrorCode::Conflict,
            message: "field revision changed".into(),
            key: Some(Box::new(key.clone())),
            actual_revision: Some(actual_revision),
        });
    }
    let serialized = value
        .map(serde_json::to_string)
        .transpose()
        .map_err(|error| StoreError::new(ErrorCode::InvalidRequest, error.to_string()))?;
    transaction.execute(
        "INSERT INTO fields (scope,entity,field,value,revision) VALUES (?1,?2,?3,?4,?5)
         ON CONFLICT(scope,entity,field) DO UPDATE SET value=excluded.value,revision=excluded.revision",
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
    *bytes += serde_json::to_vec(&record)
        .map_err(|error| StoreError::new(ErrorCode::Database, error.to_string()))?
        .len()
        + 1;
    if records.len() >= MAX_RECORDS || *bytes > MAX_RESPONSE_BYTES {
        return Err(StoreError::new(ErrorCode::Limit, "response too large"));
    }
    records.push(record);
    Ok(())
}

pub fn execute(connection: &mut Connection, request: &Request) -> Result<Response, StoreError> {
    validate(request)?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(database)?;
    let previous_revision: u64 = transaction
        .query_row("SELECT revision FROM metadata WHERE id=1", [], |row| {
            row_revision(row, 0)
        })
        .map_err(database)?;
    let has_writes = request
        .operations
        .iter()
        .any(|operation| !matches!(operation, Operation::Read { .. }));
    let revision = if has_writes {
        previous_revision
            .checked_add(1)
            .filter(|value| *value <= i64::MAX as u64)
            .ok_or_else(|| StoreError::new(ErrorCode::Limit, "revision exhausted"))?
    } else {
        previous_revision
    };
    let mut records = Vec::new();
    let mut record_bytes = 0;
    for operation in &request.operations {
        match operation {
            Operation::Read { scope } => {
                let scope_text = scope_id(scope)?;
                let mut statement = transaction.prepare("SELECT entity,field,value,revision FROM fields WHERE scope=?1 ORDER BY entity,field LIMIT ?2").map_err(database)?;
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
                        &mut records,
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
                &mut records,
                &mut record_bytes,
                write(&transaction, key, Some(value), *expected_revision, revision)?,
            )?,
            Operation::Delete {
                key,
                expected_revision,
            } => append_record(
                &mut records,
                &mut record_bytes,
                write(&transaction, key, None, *expected_revision, revision)?,
            )?,
        }
    }
    let response = Response {
        version: PROTOCOL_VERSION,
        request_id: request.request_id,
        revision,
        records,
        error: None,
    };
    if serde_json::to_vec(&response)
        .map_err(|error| StoreError::new(ErrorCode::Database, error.to_string()))?
        .len()
        > MAX_RESPONSE_BYTES
    {
        return Err(StoreError::new(ErrorCode::Limit, "response too large"));
    }
    if has_writes {
        transaction
            .execute(
                "UPDATE metadata SET revision=?1 WHERE id=1",
                [revision as i64],
            )
            .map_err(database)?;
    }
    transaction.commit().map_err(database)?;
    Ok(response)
}
