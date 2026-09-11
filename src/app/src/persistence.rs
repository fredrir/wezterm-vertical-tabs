use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};
use vtabs_core::{Error, Folder, Model, Space, SpaceTemplate};
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
    request_id: u64,
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
            request_id: 0,
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
        if let Some(scope) = &self.session
            && !model.private
        {
            self.reads.push(scope.clone());
            self.deadline = Some(Duration::ZERO);
        }
    }
    pub fn observe(&mut self, model: &Model, now: Duration) {
        let mut values = profile_values(model, &self.profile);
        if !model.private
            && let Some(scope) = &self.session
        {
            values.extend(session_values(model, scope, self.window_id));
        }
        self.observe_values(values, now, false);
    }
    pub fn observe_session(&mut self, model: &Model, now: Duration) {
        if model.private {
            return;
        }
        if let Some(scope) = &self.session {
            self.observe_values(session_values(model, scope, self.window_id), now, true);
        }
    }
    pub fn restore_discovered(&mut self, model: &mut Model) -> Result<bool, Error> {
        if model.private {
            return Ok(false);
        }
        let Some(scope) = &self.session else {
            return Ok(false);
        };
        let mut changed = false;
        for (key, record) in &self.known {
            if &key.scope != scope || key.field != "membership" || self.touched.contains(key) {
                continue;
            }
            let Some(id) = key
                .entity
                .strip_prefix("tab:")
                .and_then(|id| id.parse::<u64>().ok())
            else {
                continue;
            };
            if !model.tabs.contains_key(&id) {
                continue;
            }
            let Some(value) = &record.value else { continue };
            let Some(space) = value.get("space").and_then(Value::as_str) else {
                continue;
            };
            changed |= model.restore_tab_membership(
                id,
                space,
                value
                    .get("manual")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                value
                    .get("pinned")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                value.get("folder").and_then(Value::as_str),
            )?;
        }
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
    pub fn take_request(&mut self, model: &Model, now: Duration) -> Option<Request> {
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
            if model.private {
                self.dirty
                    .retain(|key, _| matches!(key.scope, Scope::Profile { .. }));
            }
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
        self.request_id = REQUEST_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut request = Request::new(self.request_id, operations.clone());
        // Public catalog edits contain no live private data and remain profile-scoped.
        request.private = model.private
            && request
                .operations
                .iter()
                .all(|op| matches!(op, Operation::Read { .. }));
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
            // Catalog order is one semantic field, but simultaneous creation in two GUI
            // clients must retain both new IDs. Local reorder wins for existing entries;
            // append remote additions unless this client explicitly deleted that space.
            for (field, prefix) in [("order", "space"), ("folder_order", "folder")] {
                let key = Key {
                    scope: self.profile.clone(),
                    entity: "catalog".into(),
                    field: field.into(),
                };
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
                        let name = Key {
                            scope: self.profile.clone(),
                            entity: format!("{prefix}:{id}"),
                            field: "name".into(),
                        };
                        !self
                            .known
                            .get(&name)
                            .is_some_and(|record| record.value.is_none())
                            || matches!(self.dirty.get(&name), Some(Some(_)))
                    });
                    for id in remote.iter().filter_map(Value::as_str) {
                        let deleted = self.dirty.get(&Key {
                            scope: self.profile.clone(),
                            entity: format!("{prefix}:{id}"),
                            field: "name".into(),
                        }) == Some(&None);
                        if !deleted && !order.iter().any(|v| v.as_str() == Some(id)) {
                            order.push(json!(id));
                        }
                    }
                    let value = Value::Array(order);
                    self.dirty.insert(key.clone(), Some(value.clone()));
                    self.observed.insert(key, value);
                }
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
        let mut changed = false;
        {
            let order_key = Key {
                scope: self.profile.clone(),
                entity: "catalog".into(),
                field: "order".into(),
            };
            if let Some(order) = values.get(&order_key).and_then(Value::as_array) {
                let mut spaces = Vec::new();
                for id in order.iter().filter_map(Value::as_str) {
                    let entity = format!("space:{id}");
                    let get = |field: &str| {
                        values.get(&Key {
                            scope: self.profile.clone(),
                            entity: entity.clone(),
                            field: field.into(),
                        })
                    };
                    let mut space =
                        Space::new(id, get("name").and_then(Value::as_str).unwrap_or(id));
                    if let Some(icon) = get("icon").and_then(Value::as_str) {
                        space.icon = icon.into();
                    }
                    space.accent = get("accent").and_then(Value::as_str).map(str::to_owned);
                    if let Some(rules) = get("rules") {
                        space.rules = serde_json::from_value(rules.clone())
                            .map_err(|e| Error(e.to_string()))?;
                    }
                    space.template = get("template").and_then(Value::as_str).map(str::to_owned);
                    spaces.push(space);
                }
                let templates = values
                    .get(&Key {
                        scope: self.profile.clone(),
                        entity: "catalog".into(),
                        field: "templates".into(),
                    })
                    .map(|v| serde_json::from_value::<Vec<SpaceTemplate>>(v.clone()))
                    .transpose()
                    .map_err(|e| Error(e.to_string()))?
                    .unwrap_or_default();
                if !spaces.is_empty() && (spaces != model.spaces || templates != model.templates) {
                    model.load_catalog(spaces, templates)?;
                    changed = true;
                }
            }
        }
        let folder_order_key = Key {
            scope: self.profile.clone(),
            entity: "catalog".into(),
            field: "folder_order".into(),
        };
        if let Some(value) = values.get(&folder_order_key) {
            let order = value
                .as_array()
                .ok_or_else(|| Error("Invalid folder order".into()))?;
            if order.len() > 256 {
                return Err(Error("At most 256 folders are supported".into()));
            }
            let mut folders = Vec::with_capacity(order.len());
            for value in order {
                let id = value
                    .as_str()
                    .ok_or_else(|| Error("Invalid folder ID".into()))?;
                let get = |field: &str| {
                    values.get(&Key {
                        scope: self.profile.clone(),
                        entity: format!("folder:{id}"),
                        field: field.into(),
                    })
                };
                let name = get("name")
                    .and_then(Value::as_str)
                    .ok_or_else(|| Error("Folder name is missing".into()))?;
                let space = get("space")
                    .and_then(Value::as_str)
                    .ok_or_else(|| Error("Folder space is missing".into()))?;
                // A simultaneous space deletion wins over an older folder catalog entry.
                if !model.spaces.iter().any(|item| item.id == space) {
                    continue;
                }
                let collapsed = get("collapsed")
                    .map(|value| {
                        value
                            .as_bool()
                            .ok_or_else(|| Error("Invalid folder collapse state".into()))
                    })
                    .transpose()?
                    .unwrap_or(false);
                folders.push(Folder {
                    id: id.into(),
                    name: name.into(),
                    space_id: space.into(),
                    collapsed,
                });
            }
            if folders != model.folders {
                model.load_folders(folders)?;
                changed = true;
            }
        }
        let preferences = values
            .iter()
            .filter(|(key, _)| key.scope == self.profile && key.entity == "settings")
            .map(|(key, value)| (key.field.clone(), value.clone()))
            .collect::<BTreeMap<_, _>>();
        if &preferences != model.persisted_settings() {
            model.load_preferences(preferences)?;
            changed = true;
        }
        if !model.private
            && let Some(scope) = &self.session
        {
            for (key, value) in &values {
                if &key.scope != scope || key.field != "membership" {
                    continue;
                }
                let Some(id) = key
                    .entity
                    .strip_prefix("tab:")
                    .and_then(|id| id.parse::<u64>().ok())
                else {
                    continue;
                };
                if !model.tabs.contains_key(&id) {
                    continue;
                }
                let Some(space) = value.get("space").and_then(Value::as_str) else {
                    continue;
                };
                let manual = value
                    .get("manual")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let pinned = value
                    .get("pinned")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                changed |= model.restore_tab_membership(
                    id,
                    space,
                    manual,
                    pinned,
                    value.get("folder").and_then(Value::as_str),
                )?;
            }
        }
        if !model.private
            && let (Some(scope), Some(id)) = (&self.session, self.window_id)
            && let Some(space) = values
                .get(&Key {
                    scope: scope.clone(),
                    entity: format!("window:{id}"),
                    field: "selected_space".into(),
                })
                .and_then(Value::as_str)
            && model.spaces.iter().any(|s| s.id == space)
            && model.selected_space != space
        {
            model.dispatch(vtabs_core::Intent::SelectSpace(space.into()))?;
            changed = true;
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

fn profile_values(model: &Model, scope: &Scope) -> BTreeMap<Key, Value> {
    let mut values = BTreeMap::new();
    let mut insert = |entity: &str, field: &str, value: Value| {
        values.insert(
            Key {
                scope: scope.clone(),
                entity: entity.into(),
                field: field.into(),
            },
            value,
        );
    };
    insert(
        "catalog",
        "order",
        json!(model.spaces.iter().map(|s| &s.id).collect::<Vec<_>>()),
    );
    insert("catalog", "templates", json!(model.templates));
    insert(
        "catalog",
        "folder_order",
        json!(
            model
                .folders
                .iter()
                .map(|folder| &folder.id)
                .collect::<Vec<_>>()
        ),
    );
    for folder in &model.folders {
        let entity = format!("folder:{}", folder.id);
        insert(&entity, "name", json!(folder.name));
        insert(&entity, "space", json!(folder.space_id));
        insert(&entity, "collapsed", json!(folder.collapsed));
    }
    for space in &model.spaces {
        let entity = format!("space:{}", space.id);
        insert(&entity, "name", json!(space.name));
        insert(&entity, "icon", json!(space.icon));
        insert(&entity, "accent", json!(space.accent));
        insert(&entity, "rules", json!(space.rules));
        insert(&entity, "template", json!(space.template));
    }
    for (key, value) in model.persisted_settings() {
        insert("settings", key, value.clone());
    }
    values
}
fn session_values(model: &Model, scope: &Scope, window_id: Option<u64>) -> BTreeMap<Key, Value> {
    let mut values: BTreeMap<Key, Value> = model
        .tabs
        .values()
        .filter(|tab| tab.manual_assignment || tab.pinned || tab.folder_id.is_some())
        .map(|tab| {
            (
                Key {
                    scope: scope.clone(),
                    entity: format!("tab:{}", tab.id),
                    field: "membership".into(),
                },
                json!({"space":tab.space_id,"manual":tab.manual_assignment,"pinned":tab.pinned,"folder":tab.folder_id}),
            )
        })
        .collect();
    if let Some(id) = window_id {
        values.insert(
            Key {
                scope: scope.clone(),
                entity: format!("window:{id}"),
                field: "selected_space".into(),
            },
            json!(model.selected_space),
        );
    }
    values
}
