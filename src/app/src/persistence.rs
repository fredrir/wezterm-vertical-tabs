mod codec;

use crate::{Update, WindowApp};
use codec::{Field, profile_values};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};
use vtabs_core::{Error, Model};
use vtabs_store::{
    Key, MAX_OPERATIONS, Operation, PROTOCOL_VERSION, Record, Request, Response, Scope,
};

const WRITE_DELAY: Duration = Duration::from_millis(100);
const MAX_DIRTY: usize = 4096;
const MAX_RETRIES: u8 = 3;

struct Flight {
    id: u64,
    operations: Vec<Operation>,
}
pub(crate) struct Persistence {
    profile: Scope,
    known: BTreeMap<Key, Record>,
    observed: BTreeMap<Key, Value>,
    dirty: BTreeMap<Key, Option<Value>>,
    touched: BTreeSet<Key>,
    read: bool,
    flight: Option<Flight>,
    deadline: Option<Duration>,
    failures: u8,
}

impl Persistence {
    pub fn new(model: &Model) -> Self {
        let profile = Scope::profile(&model.profile);
        Self {
            observed: profile_values(model, &profile),
            profile,
            known: BTreeMap::new(),
            dirty: BTreeMap::new(),
            touched: BTreeSet::new(),
            read: true,
            flight: None,
            deadline: Some(Duration::ZERO),
            failures: 0,
        }
    }
    pub fn deadline(&self) -> Option<Duration> {
        if self.flight.is_none() {
            self.deadline
        } else {
            None
        }
    }
    pub fn pending(&self) -> bool {
        self.flight.is_some() || !self.dirty.is_empty() || self.read
    }
    fn reading(flight: &Flight) -> bool {
        flight
            .operations
            .iter()
            .any(|op| matches!(op, Operation::Read { .. }))
    }
    pub fn refresh(&mut self, now: Duration) {
        if !self.flight.as_ref().is_some_and(Self::reading) {
            self.read = true;
        }
        self.failures = 0;
        if self.read {
            self.deadline = Some(now);
        }
    }
    pub fn observe(&mut self, model: &Model, now: Duration) {
        let values = profile_values(model, &self.profile);
        for (key, value) in &values {
            if self.observed.get(key) != Some(value)
                || self.known.get(key).and_then(|r| r.value.as_ref()) != Some(value)
            {
                self.dirty.insert(key.clone(), Some(value.clone()));
                self.touched.insert(key.clone());
            }
        }
        for key in self.observed.keys() {
            if !values.contains_key(key) {
                self.dirty.insert(key.clone(), None);
                self.touched.insert(key.clone());
            }
        }
        self.observed = values;
        if !self.dirty.is_empty() {
            self.deadline = Some(now + WRITE_DELAY);
        }
    }
    pub fn take_request(&mut self, now: Duration) -> Option<Request> {
        if self.flight.is_some()
            || self.failures >= MAX_RETRIES
            || self.deadline.is_some_and(|d| d > now)
        {
            return None;
        }
        let operations = if std::mem::take(&mut self.read) {
            vec![Operation::Read {
                scope: self.profile.clone(),
            }]
        } else {
            self.dirty
                .iter()
                .take(MAX_OPERATIONS)
                .map(|(key, value)| {
                    let revision = Some(self.known.get(key).map_or(0, |r| r.revision));
                    match value {
                        Some(value) => Operation::Put {
                            key: key.clone(),
                            value: value.clone(),
                            expected_revision: revision,
                        },
                        None => Operation::Delete {
                            key: key.clone(),
                            expected_revision: revision,
                        },
                    }
                })
                .collect::<Vec<_>>()
        };
        if operations.is_empty() {
            self.deadline = None;
            return None;
        }
        if self.dirty.len() > MAX_DIRTY {
            self.failures = MAX_RETRIES;
            return None;
        }
        static REQUEST_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let request_id = REQUEST_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let request = Request::new(request_id, operations.clone());
        self.flight = Some(Flight {
            id: request.request_id,
            operations,
        });
        self.deadline = None;
        Some(request)
    }
    pub fn complete(
        &mut self,
        model: &mut Model,
        response: Response,
        now: Duration,
    ) -> Result<bool, Error> {
        let Some(flight) = self.flight.as_ref() else {
            return Ok(false);
        };
        if response.request_id != flight.id {
            return Ok(false);
        }
        if response.version != PROTOCOL_VERSION {
            self.failed(response.request_id, now);
            return Err(Error("Unsupported storage protocol".into()));
        }
        if let Some(error) = response.error {
            self.failed(response.request_id, now);
            if error.code == vtabs_store::ErrorCode::Conflict {
                self.read = true;
            }
            return Err(Error(format!("Storage: {}", error.message)));
        }
        let flight = self.flight.take().unwrap();
        let read = Self::reading(&flight);
        for record in response.records {
            if record.key.scope == self.profile {
                self.known.insert(record.key.clone(), record);
            }
        }
        if read {
            // Local reorders win, remote additions append unless this client deleted that folder.
            let key = Field::FolderOrder.key(&self.profile);
            if let (Some(Some(local)), Some(remote)) = (
                self.dirty.get(&key),
                self.known.get(&key).and_then(|r| r.value.as_ref()),
            ) && let (Some(local), Some(remote)) = (local.as_array(), remote.as_array())
            {
                let mut order = local.clone();
                order.retain(|value| {
                    let Some(id) = value.as_str() else {
                        return true;
                    };
                    let name = Field::FolderName(id).key(&self.profile);
                    !self
                        .known
                        .get(&name)
                        .is_some_and(|record| record.value.is_none())
                        || matches!(self.dirty.get(&name), Some(Some(_)))
                });
                for id in remote.iter().filter_map(Value::as_str) {
                    let deleted =
                        self.dirty.get(&Field::FolderName(id).key(&self.profile)) == Some(&None);
                    if !deleted && !order.iter().any(|v| v.as_str() == Some(id)) {
                        order.push(json!(id));
                    }
                }
                let value = Value::Array(order);
                self.dirty.insert(key.clone(), Some(value.clone()));
                self.observed.insert(key, value);
            }
        }
        let changed = if read {
            let mut candidate = model.clone();
            let changed = match self.restore(&mut candidate) {
                Ok(changed) => changed,
                Err(error) => {
                    self.flight = Some(flight);
                    self.failed(response.request_id, now);
                    return Err(error);
                }
            };
            if changed {
                *model = candidate;
            }
            changed
        } else {
            for operation in flight.operations {
                let (key, written) = match operation {
                    Operation::Put { key, value, .. } => (key, Some(value)),
                    Operation::Delete { key, .. } => (key, None),
                    Operation::Read { .. } => continue,
                };
                if self.dirty.get(&key) == Some(&written) {
                    self.dirty.remove(&key);
                    self.touched.remove(&key);
                }
            }
            false
        };
        self.failures = 0;
        self.deadline = (!self.dirty.is_empty() || self.read).then_some(now + WRITE_DELAY);
        Ok(changed)
    }
    fn restore(&mut self, model: &mut Model) -> Result<bool, Error> {
        let mut values = self
            .known
            .iter()
            .filter(|(key, _)| !self.touched.contains(*key))
            .filter_map(|(key, record)| record.value.clone().map(|v| (key.clone(), v)))
            .collect::<BTreeMap<_, _>>();
        // Local feature changes made while the helper was running win field by field.
        values.extend(
            self.observed
                .iter()
                .filter(|(key, _)| self.touched.contains(*key))
                .map(|(k, v)| (k.clone(), v.clone())),
        );
        let changed = codec::restore_profile(model, &self.profile, &values)?;
        self.observed = profile_values(model, &self.profile);
        Ok(changed)
    }
    pub fn failed(&mut self, id: u64, now: Duration) {
        if self.flight.as_ref().is_none_or(|f| f.id != id) {
            return;
        }
        let flight = self.flight.take().unwrap();
        self.read |= Self::reading(&flight);
        self.failures = self.failures.saturating_add(1);
        self.deadline = (self.failures < MAX_RETRIES)
            .then_some(now + Duration::from_secs(1u64 << self.failures));
    }
    pub fn retry(&mut self, now: Duration) {
        self.failures = 0;
        if self.pending() {
            self.deadline = Some(now);
        }
    }
}

impl WindowApp {
    pub fn storage_deadline(&self) -> Option<Duration> {
        self.storage.deadline()
    }
    pub fn refresh_storage(&mut self) {
        self.storage.refresh(self.now);
    }
    pub fn take_storage_request(&mut self, now: Duration) -> Option<Request> {
        self.now = now;
        self.storage.take_request(now)
    }
    pub fn complete_storage(&mut self, response: Response) -> Result<Update, Error> {
        if self.storage.complete(&mut self.model, response, self.now)? {
            self.ui.invalidate();
        }
        Ok(Update::default())
    }
    pub fn storage_failed(&mut self, request_id: u64, message: impl Into<String>) {
        let message = message.into();
        self.storage.failed(request_id, self.now);
        if !self.errors.contains(&message) {
            self.errors.push(message);
        }
    }
    pub fn retry_storage(&mut self) {
        self.storage.retry(self.now);
    }
    pub fn restart_storage_after_cancel(&mut self, request_id: u64) {
        self.storage.failed(request_id, self.now);
        self.storage.retry(self.now);
    }
    pub fn storage_pending(&self) -> bool {
        self.storage.pending()
    }
}
