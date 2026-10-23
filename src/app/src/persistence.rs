mod codec;
mod tabs;

use crate::{Update, WindowApp};
use codec::{Field, profile_values};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};
use vtabs_core::{Error, Model, TabId, TabIdentity};
use vtabs_store::{
    Key, MAX_OPERATIONS, Operation, PROTOCOL_VERSION, Record, Request, Response, Scope,
};

const WRITE_DELAY: Duration = Duration::from_millis(100);
const MAX_RETRIES: u8 = 3;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Read {
    Profile(Scope),
    Tab { scope: Scope, entity: String },
}

impl Read {
    fn for_key(key: &Key) -> Self {
        match &key.scope {
            Scope::Profile { .. } => Self::Profile(key.scope.clone()),
            Scope::MuxSession { .. } => Self::Tab {
                scope: key.scope.clone(),
                entity: key.entity.clone(),
            },
        }
    }

    fn from_operation(operation: &Operation) -> Option<Self> {
        match operation {
            Operation::Read { scope } => Some(Self::Profile(scope.clone())),
            Operation::ReadKey { key } => Some(Self::for_key(key)),
            _ => None,
        }
    }

    fn operations(&self) -> Vec<Operation> {
        match self {
            Self::Profile(scope) => vec![Operation::Read {
                scope: scope.clone(),
            }],
            Self::Tab { scope, entity } => tabs::FIELDS
                .into_iter()
                .map(|field| Operation::ReadKey {
                    key: Key {
                        scope: scope.clone(),
                        entity: entity.clone(),
                        field: field.into(),
                    },
                })
                .collect(),
        }
    }
}

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
    targets: BTreeSet<Read>,
    loaded: BTreeSet<Read>,
    read: BTreeSet<Read>,
    tabs: BTreeMap<TabId, Vec<TabIdentity>>,
    flight: Option<Flight>,
    deadline: Option<Duration>,
    failures: u8,
}

impl Persistence {
    pub fn new(model: &Model) -> Self {
        let profile = Scope::profile(&model.profile);
        Self {
            observed: profile_values(model, &profile),
            targets: BTreeSet::from([Read::Profile(profile.clone())]),
            loaded: BTreeSet::new(),
            read: BTreeSet::from([Read::Profile(profile.clone())]),
            tabs: BTreeMap::new(),
            profile,
            known: BTreeMap::new(),
            dirty: BTreeMap::new(),
            touched: BTreeSet::new(),
            flight: None,
            deadline: Some(Duration::ZERO),
            failures: 0,
        }
    }
    pub fn deleted_folders(&self) -> Vec<String> {
        self.dirty
            .iter()
            .filter_map(|(key, value)| {
                (key.scope == self.profile && key.field == "name" && value.is_none())
                    .then(|| key.entity.strip_prefix("folder:").map(str::to_owned))
                    .flatten()
            })
            .collect()
    }
    pub fn from_transfer(model: &Model, deleted_folders: &[String]) -> Self {
        let mut storage = Self::new(model);
        // A transfer carries explicit GUI state, not provisional startup defaults.
        // Protect it even when the source helper has not committed its latest edits.
        storage.observed.extend(tabs::values(model));
        storage.dirty = storage
            .observed
            .iter()
            .map(|(key, value)| (key.clone(), Some(value.clone())))
            .collect();
        storage.touched.extend(storage.observed.keys().cloned());
        for id in deleted_folders {
            for field in [
                Field::FolderName(id),
                Field::FolderSpace(id),
                Field::FolderCollapsed(id),
            ] {
                let key = field.key(&storage.profile);
                storage.dirty.insert(key.clone(), None);
                storage.touched.insert(key);
            }
        }
        storage.tabs = model
            .tabs
            .iter()
            .map(|(id, tab)| (*id, tab.identities.clone()))
            .collect();
        storage.register_tabs(model, Duration::ZERO);
        storage
    }
    pub fn deadline(&self) -> Option<Duration> {
        if self.flight.is_none() {
            self.deadline
        } else {
            None
        }
    }
    pub fn pending(&self) -> bool {
        self.flight.is_some() || !self.dirty.is_empty() || !self.read.is_empty()
    }
    fn reading(flight: &Flight) -> bool {
        flight
            .operations
            .iter()
            .any(|op| Read::from_operation(op).is_some())
    }
    pub fn refresh(&mut self, now: Duration) {
        let in_flight = self
            .flight
            .iter()
            .flat_map(|flight| &flight.operations)
            .filter_map(Read::from_operation)
            .collect::<BTreeSet<_>>();
        self.read.extend(
            self.targets
                .iter()
                .filter(|scope| !in_flight.contains(scope))
                .cloned(),
        );
        self.failures = 0;
        if !self.read.is_empty() {
            self.deadline = Some(now);
        }
    }
    /// Arrival IDs are local to a GUI; only the mux identities select saved placement.
    pub fn reconcile_tabs(&mut self, model: &mut Model, now: Duration) -> Result<(), Error> {
        let arrived = model
            .tabs
            .keys()
            .filter(|id| !self.tabs.contains_key(id))
            .copied()
            .collect();
        // A pane joining an existing GUI tab inherits that tab's established placement.
        // Its old database values must not become authoritative when a delayed read arrives.
        let joined = model
            .tabs
            .iter()
            .flat_map(|(id, tab)| {
                self.tabs.get(id).into_iter().flat_map(|previous| {
                    tab.identities
                        .iter()
                        .filter(|identity| !previous.contains(identity))
                })
            })
            .map(|identity| Read::Tab {
                scope: tabs::scope(&model.profile, identity),
                entity: identity.entity.clone(),
            })
            .collect::<BTreeSet<_>>();
        for (key, value) in tabs::values(model) {
            if joined.contains(&Read::for_key(&key)) {
                self.dirty.insert(key.clone(), Some(value.clone()));
                self.touched.insert(key.clone());
                self.observed.insert(key, value);
                self.deadline = Some(now + WRITE_DELAY);
            }
        }
        self.tabs = model
            .tabs
            .iter()
            .map(|(id, tab)| (*id, tab.identities.clone()))
            .collect();
        self.register_tabs(model, now);
        let values = self.restore_values();
        tabs::restore(model, &values, Some(&arrived))?;
        self.observe_host_tabs(model, now);
        Ok(())
    }
    pub fn observe_host_tabs(&mut self, model: &Model, now: Duration) {
        // Provisional routing must not overwrite saved membership before the first read.
        for (key, value) in tabs::values(model) {
            if !self.loaded.contains(&Read::for_key(&key)) && !self.touched.contains(&key) {
                self.observed.insert(key, value);
            } else {
                self.observed.entry(key).or_insert(value);
            }
        }
        self.observe_tabs(model, now);
    }
    fn register_tabs(&mut self, model: &Model, now: Duration) {
        let targets = std::iter::once(Read::Profile(self.profile.clone()))
            .chain(
                model
                    .tabs
                    .values()
                    .flat_map(|tab| &tab.identities)
                    .map(|identity| Read::Tab {
                        scope: tabs::scope(&model.profile, identity),
                        entity: identity.entity.clone(),
                    }),
            )
            .collect::<BTreeSet<_>>();
        for target in targets.difference(&self.targets) {
            self.read.insert(target.clone());
            self.deadline = Some(now);
        }
        self.targets = targets;
        // Retain pending detached writes, but never scan historical mux entities on refresh.
        self.read.retain(|target| {
            self.targets.contains(target)
                || self.dirty.keys().any(|key| Read::for_key(key) == *target)
        });
    }
    fn observe_tabs(&mut self, model: &Model, now: Duration) {
        self.register_tabs(model, now);
        let mut edited = false;
        for (key, value) in tabs::values(model) {
            let changed = self.observed.get(&key) != Some(&value);
            let unsaved = self.loaded.contains(&Read::for_key(&key))
                && self
                    .known
                    .get(&key)
                    .and_then(|record| record.value.as_ref())
                    != Some(&value);
            if changed || unsaved {
                edited |= self.dirty.get(&key) != Some(&Some(value.clone()));
                self.dirty.insert(key.clone(), Some(value.clone()));
                self.touched.insert(key.clone());
            }
            self.observed.insert(key, value);
        }
        // Disappearance can be a detach or a move to another window, not a closed shell.
        if edited && self.read.is_empty() {
            self.deadline = Some(now + WRITE_DELAY);
        }
    }
    pub fn observe(&mut self, model: &Model, now: Duration) {
        let values = profile_values(model, &self.profile);
        for (key, value) in &values {
            if self.observed.get(key) != Some(value)
                || (self.loaded.contains(&Read::Profile(self.profile.clone()))
                    && self.known.get(key).and_then(|r| r.value.as_ref()) != Some(value))
            {
                self.dirty.insert(key.clone(), Some(value.clone()));
                self.touched.insert(key.clone());
            }
        }
        for key in self.observed.keys() {
            if key.scope == self.profile && !values.contains_key(key) {
                self.dirty.insert(key.clone(), None);
                self.touched.insert(key.clone());
            }
        }
        self.observed.retain(|key, _| key.scope != self.profile);
        self.observed.extend(values);
        self.observe_tabs(model, now);
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
        let operations = if !self.read.is_empty() {
            let mut operations = Vec::new();
            let mut targets = Vec::new();
            for target in &self.read {
                // Read a membership together: restoring its folder before its space
                // could otherwise clear a valid folder and persist that partial result.
                let reads = target.operations();
                if operations.len() + reads.len() > MAX_OPERATIONS {
                    break;
                }
                operations.extend(reads);
                targets.push(target.clone());
            }
            for target in targets {
                self.read.remove(&target);
            }
            operations
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
                self.read.extend(self.targets.iter().cloned());
                self.read.extend(self.dirty.keys().map(Read::for_key));
            }
            return Err(Error(format!("Storage: {}", error.message)));
        }
        let flight = self.flight.take().unwrap();
        let read = Self::reading(&flight);
        for record in response.records {
            if self.targets.contains(&Read::for_key(&record.key))
                || self.dirty.contains_key(&record.key)
            {
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
            self.loaded
                .extend(flight.operations.iter().filter_map(Read::from_operation));
            self.observe_tabs(model, now);
            changed
        } else {
            for operation in flight.operations {
                let (key, written) = match operation {
                    Operation::Put { key, value, .. } => (key, Some(value)),
                    Operation::Delete { key, .. } => (key, None),
                    Operation::Read { .. } | Operation::ReadKey { .. } => continue,
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
            (!self.dirty.is_empty() || !self.read.is_empty()).then_some(now + WRITE_DELAY);
        Ok(changed)
    }
    fn restore_values(&self) -> BTreeMap<Key, Value> {
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
        values
    }
    fn restore(&mut self, model: &mut Model) -> Result<bool, Error> {
        let values = self.restore_values();
        let mut changed = codec::restore_profile(model, &self.profile, &values)?;
        changed |= tabs::restore(model, &values, None)?;
        self.observed.retain(|key, _| key.scope != self.profile);
        self.observed.extend(profile_values(model, &self.profile));
        self.observed.extend(tabs::values(model));
        Ok(changed)
    }
    pub fn failed(&mut self, id: u64, now: Duration) {
        if self.flight.as_ref().is_none_or(|f| f.id != id) {
            return;
        }
        let flight = self.flight.take().unwrap();
        self.read
            .extend(flight.operations.iter().filter_map(Read::from_operation));
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
