//! Bounded storage messages; SQLite is only linked into the helper.

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const PROTOCOL_VERSION: u32 = 1;
pub const MAX_REQUEST_BYTES: usize = 1024 * 1024;
pub const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_OPERATIONS: usize = 128;
pub const MAX_RECORDS: usize = 4096;
pub const MAX_VALUE_BYTES: usize = 64 * 1024;
pub const MAX_KEY_BYTES: usize = 256;

/// Session scopes must use a verified incarnation, never an inferred tab identity.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Scope {
    Profile {
        profile: String,
    },
    Session {
        profile: String,
        incarnation: String,
    },
}

impl Scope {
    pub fn profile(profile: impl Into<String>) -> Self {
        Self::Profile {
            profile: profile.into(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct Key {
    pub scope: Scope,
    pub entity: String,
    pub field: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub key: Key,
    /// None is a tombstone. Keep its revision when making subsequent writes.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present_value"
    )]
    pub value: Option<Value>,
    pub revision: u64,
}

fn present_value<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Value>, D::Error> {
    Value::deserialize(deserializer).map(Some)
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    Read {
        scope: Scope,
    },
    Put {
        key: Key,
        value: Value,
        expected_revision: Option<u64>,
    },
    Delete {
        key: Key,
        expected_revision: Option<u64>,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub version: u32,
    pub request_id: u64,
    #[serde(default)]
    pub private: bool,
    pub operations: Vec<Operation>,
}

impl Request {
    pub fn new(request_id: u64, operations: Vec<Operation>) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            request_id,
            private: false,
            operations,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidRequest,
    Conflict,
    PrivateWrite,
    Database,
    NewerSchema,
    Limit,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct StoreError {
    pub code: ErrorCode,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<Box<Key>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actual_revision: Option<u64>,
}

impl StoreError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            key: None,
            actual_revision: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Response {
    pub version: u32,
    pub request_id: u64,
    pub revision: u64,
    pub records: Vec<Record>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<StoreError>,
}

impl Response {
    pub fn failure(request_id: u64, error: StoreError) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            request_id,
            revision: 0,
            records: vec![],
            error: Some(error),
        }
    }
}

#[cfg(feature = "sqlite")]
pub mod sqlite;
