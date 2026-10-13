mod codec;

use codec::{Field, profile_values, session_values};
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
    session: Option<Scope>,
    known: BTreeMap<Key, Record>,
    observed: BTreeMap<Key, Value>,
    dirty: BTreeMap<Key, Option<Value>>,
    touched: BTreeSet<Key>,
    reads: Vec<Scope>,
    flight: Option<Flight>,
    deadline: Option<Duration>,
    failures: u8,
    window_id: Option<u64>,
}

impl Persistence {
    pub fn new(model: &Model) -> Self {
        let profile = Scope::profile(&model.profile);
        Self {
            profile: profile.clone(),
            session: None,
            known: BTreeMap::new(),
            observed: profile_values(model, &profile),
            dirty: BTreeMap::new(),
            touched: BTreeSet::new(),
            reads: vec![profile],
            flight: None,
            deadline: Some(Duration::ZERO),
            failures: 0,
            window_id: None,
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
        self.flight.is_some() || !self.dirty.is_empty() || !self.reads.is_empty()
    }
    pub fn set_window_identity(&mut self, id: u64) {
        self.window_id = Some(id);
    }
    pub fn refresh(&mut self, now: Duration) {
        for scope in std::iter::once(self.profile.clone()).chain(self.session.clone()) {
            let reading = self.flight.as_ref().is_some_and(|f| {
                f.operations
                    .iter()
                    .any(|op| matches!(op,Operation::Read{scope:active}if active==&scope))
            });
            if !reading && !self.reads.contains(&scope) {
                self.reads.push(scope);
            }
        }
        self.failures = 0;
        if !self.reads.is_empty() {
            self.deadline = Some(now);
        }
    }
    pub fn set_session(&mut self, model: &Model, incarnation: Option<String>) {
        let session = incarnation
            .filter(|s| !s.is_empty() && s.len() <= 256)
            .map(|incarnation| Scope::Session {
                profile: model.profile.clone(),
                incarnation,
            });
        if session == self.session {
            return;
        }
        self.dirty
            .retain(|key, _| !matches!(key.scope, Scope::Session { .. }));
        self.observed
            .retain(|key, _| !matches!(key.scope, Scope::Session { .. }));
        self.known
            .retain(|key, _| !matches!(key.scope, Scope::Session { .. }));
        self.reads
            .retain(|scope| !matches!(scope, Scope::Session { .. }));
        self.session = session;
        if let Some(scope) = &self.session {
            self.reads.push(scope.clone());
            self.deadline = Some(Duration::ZERO);
        }
    }
    pub fn observe(&mut self, model: &Model, now: Duration) {
        let mut values = profile_values(model, &self.profile);
        if let Some(scope) = &self.session {
            values.extend(session_values(model, scope, self.window_id));
        }
        self.observe_values(values, now, false);
    }
    pub fn observe_session(&mut self, model: &Model, now: Duration) {
        if let Some(scope) = &self.session {
            self.observe_values(session_values(model, scope, self.window_id), now, true);
        }
    }
    pub fn restore_discovered(&mut self, model: &mut Model) -> Result<bool, Error> {
        let Some(scope) = &self.session else {
            return Ok(false);
        };
        let changed = codec::restore_memberships(
            model,
            scope,
            self.known
                .iter()
                .filter(|(key, _)| !self.touched.contains(*key))
                .filter_map(|(key, record)| record.value.as_ref().map(|value| (key, value))),
        )?;
        if changed {
            self.observed.retain(|key, _| &key.scope != scope);
            self.observed
                .extend(session_values(model, scope, self.window_id));
        }
        Ok(changed)
    }
    fn observe_values(&mut self, values: BTreeMap<Key, Value>, now: Duration, session_only: bool) {
        for (key, value) in &values {
            if self.observed.get(key) != Some(value)
                || (!session_only
                    && self.known.get(key).and_then(|r| r.value.as_ref()) != Some(value))
            {
                self.dirty.insert(key.clone(), Some(value.clone()));
                self.touched.insert(key.clone());
            }
        }
        for key in self.observed.keys() {
            if (!session_only || matches!(key.scope, Scope::Session { .. }))
                && !values.contains_key(key)
            {
                self.dirty.insert(key.clone(), None);
                self.touched.insert(key.clone());
            }
        }
        if session_only {
            self.observed
                .retain(|key, _| !matches!(key.scope, Scope::Session { .. }));
            self.observed.extend(values);
        } else {
            self.observed = values;
        }
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
        let operations = if !self.reads.is_empty() {
            self.reads
                .drain(..)
                .map(|scope| Operation::Read { scope })
                .collect()
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
            let id = response.request_id;
            self.failed(id, now);
            if error.code == vtabs_store::ErrorCode::Conflict {
                self.reads.push(self.profile.clone());
                if let Some(scope) = &self.session {
                    self.reads.push(scope.clone());
                }
            }
            return Err(Error(format!("Storage: {}", error.message)));
        }
        let flight = self.flight.take().unwrap();
        let read = flight
            .operations
            .iter()
            .any(|o| matches!(o, Operation::Read { .. }));
        for record in response.records {
            if record.key.scope == self.profile || self.session.as_ref() == Some(&record.key.scope)
            {
                self.known.insert(record.key.clone(), record);
            }
        }
        if read {
            // Folder order is one semantic field, but simultaneous creation in two GUI
            // clients must retain both new IDs. Local reorder wins for existing entries;
            // append remote additions unless this client explicitly deleted that folder.
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
        self.deadline =
            (!self.dirty.is_empty() || !self.reads.is_empty()).then_some(now + WRITE_DELAY);
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
        let mut changed = codec::restore_profile(model, &self.profile, &values)?;
        if let Some(scope) = &self.session {
            changed |= codec::restore_session(model, scope, self.window_id, &values)?;
        }
        self.observed = profile_values(model, &self.profile);
        if let Some(scope) = &self.session {
            self.observed
                .extend(session_values(model, scope, self.window_id));
        }
        Ok(changed)
    }
    pub fn failed(&mut self, id: u64, now: Duration) {
        if self.flight.as_ref().is_none_or(|f| f.id != id) {
            return;
        }
        let flight = self.flight.take().unwrap();
        for op in flight.operations {
            if let Operation::Read { scope } = op
                && !self.reads.contains(&scope)
            {
                self.reads.push(scope);
            }
        }
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
