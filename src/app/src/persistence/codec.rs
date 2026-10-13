use serde_json::{Value, json};
use std::collections::BTreeMap;
use vtabs_core::{Error, Folder, Model, Space, Tab, TabId};
use vtabs_store::{Key, Scope};

const MEMBERSHIP: &str = "membership";

/// Application fields within the store's generic entity/field protocol.
pub(super) enum Field<'a> {
    DerivedSpaces,
    FolderOrder,
    FolderName(&'a str),
    FolderSpace(&'a str),
    FolderCollapsed(&'a str),
    SpaceCollapsed(&'a str),
    TabMembership(TabId),
    WindowSelection(u64),
}

impl Field<'_> {
    pub(super) fn key(self, scope: &Scope) -> Key {
        let (entity, field) = match self {
            Self::DerivedSpaces => ("catalog".into(), "derived"),
            Self::FolderOrder => ("catalog".into(), "folder_order"),
            Self::FolderName(id) => (format!("folder:{id}"), "name"),
            Self::FolderSpace(id) => (format!("folder:{id}"), "space"),
            Self::FolderCollapsed(id) => (format!("folder:{id}"), "collapsed"),
            Self::SpaceCollapsed(id) => (format!("space:{id}"), "collapsed"),
            Self::TabMembership(id) => (format!("tab:{id}"), MEMBERSHIP),
            Self::WindowSelection(id) => (format!("window:{id}"), "selected_space"),
        };
        Key {
            scope: scope.clone(),
            entity,
            field: field.into(),
        }
    }
}

struct Membership<'a> {
    space: &'a str,
    manual: bool,
    pinned: bool,
    folder: Option<&'a str>,
}

impl<'a> Membership<'a> {
    fn from_tab(tab: &'a Tab) -> Self {
        Self {
            space: &tab.space_id,
            manual: tab.manual_assignment,
            pinned: tab.pinned,
            folder: tab.folder_id.as_deref(),
        }
    }

    fn decode(value: &'a Value) -> Option<Self> {
        Some(Self {
            space: value.get("space")?.as_str()?,
            // Older records may omit optional fields. Preserve the same defaults
            // for malformed optional values when restoring either early or late.
            manual: value
                .get("manual")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            pinned: value
                .get("pinned")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            folder: value.get("folder").and_then(Value::as_str),
        })
    }

    fn explicit(&self) -> bool {
        self.manual || self.pinned || self.folder.is_some()
    }

    fn encode(&self) -> Value {
        json!({"space":self.space,"manual":self.manual,"pinned":self.pinned,"folder":self.folder})
    }

    fn restore(self, model: &mut Model, id: TabId) -> Result<bool, Error> {
        model.restore_tab_membership(id, self.space, self.manual, self.pinned, self.folder)
    }
}

pub(super) fn profile_values(model: &Model, scope: &Scope) -> BTreeMap<Key, Value> {
    let mut values = BTreeMap::from([
        (
            Field::DerivedSpaces.key(scope),
            json!(
                model
                    .spaces
                    .iter()
                    .filter(|space| space.template.is_some())
                    .map(|space| Space {
                        collapsed: false,
                        ..space.clone()
                    })
                    .collect::<Vec<_>>()
            ),
        ),
        (
            Field::FolderOrder.key(scope),
            json!(
                model
                    .folders
                    .iter()
                    .map(|folder| &folder.id)
                    .collect::<Vec<_>>()
            ),
        ),
    ]);
    for folder in &model.folders {
        values.insert(Field::FolderName(&folder.id).key(scope), json!(folder.name));
        values.insert(
            Field::FolderSpace(&folder.id).key(scope),
            json!(folder.space_id),
        );
        values.insert(
            Field::FolderCollapsed(&folder.id).key(scope),
            json!(folder.collapsed),
        );
    }
    for space in &model.spaces {
        values.insert(
            Field::SpaceCollapsed(&space.id).key(scope),
            json!(space.collapsed),
        );
    }
    values
}

pub(super) fn session_values(
    model: &Model,
    scope: &Scope,
    window_id: Option<u64>,
) -> BTreeMap<Key, Value> {
    let mut values = BTreeMap::new();
    for tab in model.tabs.values() {
        let membership = Membership::from_tab(tab);
        if membership.explicit() {
            values.insert(Field::TabMembership(tab.id).key(scope), membership.encode());
        }
    }
    if let Some(id) = window_id {
        values.insert(
            Field::WindowSelection(id).key(scope),
            json!(model.selected_space),
        );
    }
    values
}

pub(super) fn restore_memberships<'a>(
    model: &mut Model,
    scope: &Scope,
    values: impl IntoIterator<Item = (&'a Key, &'a Value)>,
) -> Result<bool, Error> {
    let mut changed = false;
    for (key, value) in values {
        if &key.scope != scope || key.field != MEMBERSHIP {
            continue;
        }
        let Some(id) = key
            .entity
            .strip_prefix("tab:")
            .and_then(|id| id.parse::<TabId>().ok())
        else {
            continue;
        };
        if model.tabs.contains_key(&id)
            && let Some(membership) = Membership::decode(value)
        {
            changed |= membership.restore(model, id)?;
        }
    }
    Ok(changed)
}

pub(super) fn restore_profile(
    model: &mut Model,
    scope: &Scope,
    values: &BTreeMap<Key, Value>,
) -> Result<bool, Error> {
    let mut changed = false;
    // Spaces are declared in Lua; only derived spaces and collapse state live here.
    let mut spaces = model.spaces.clone();
    if let Some(value) = values.get(&Field::DerivedSpaces.key(scope)) {
        let derived = serde_json::from_value::<Vec<Space>>(value.clone())
            .map_err(|error| Error(error.to_string()))?;
        for space in derived {
            if space.template.is_some() && !spaces.iter().any(|item| item.id == space.id) {
                spaces.push(space);
            }
        }
    }
    for space in &mut spaces {
        if let Some(collapsed) = values
            .get(&Field::SpaceCollapsed(&space.id).key(scope))
            .and_then(Value::as_bool)
        {
            space.collapsed = collapsed;
        }
    }
    if spaces != model.spaces {
        model.load_catalog(spaces, model.templates.clone())?;
        changed = true;
    }
    if let Some(value) = values.get(&Field::FolderOrder.key(scope)) {
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
            let name = values
                .get(&Field::FolderName(id).key(scope))
                .and_then(Value::as_str)
                .ok_or_else(|| Error("Folder name is missing".into()))?;
            let space = values
                .get(&Field::FolderSpace(id).key(scope))
                .and_then(Value::as_str)
                .ok_or_else(|| Error("Folder space is missing".into()))?;
            // A simultaneous space deletion wins over an older folder catalog entry.
            if !model.spaces.iter().any(|item| item.id == space) {
                continue;
            }
            let collapsed = values
                .get(&Field::FolderCollapsed(id).key(scope))
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
    Ok(changed)
}

pub(super) fn restore_session(
    model: &mut Model,
    scope: &Scope,
    window_id: Option<u64>,
    values: &BTreeMap<Key, Value>,
) -> Result<bool, Error> {
    let mut changed = restore_memberships(model, scope, values)?;
    if let Some(id) = window_id
        && let Some(space) = values
            .get(&Field::WindowSelection(id).key(scope))
            .and_then(Value::as_str)
        && model.spaces.iter().any(|item| item.id == space)
        && model.selected_space != *space
    {
        model.dispatch(vtabs_core::Intent::SelectSpace(space.into()))?;
        changed = true;
    }
    Ok(changed)
}
