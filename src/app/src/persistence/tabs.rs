use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use vtabs_core::{Error, Intent, Model, TabId, TabIdentity};
use vtabs_store::{Key, Scope};

pub(super) const FIELDS: [&str; 4] = ["space", "manual", "pinned", "folder"];

pub(super) fn scope(profile: &str, identity: &TabIdentity) -> Scope {
    Scope::MuxSession {
        profile: profile.into(),
        session: identity.session.clone(),
    }
}

fn key(profile: &str, identity: &TabIdentity, field: &str) -> Key {
    Key {
        scope: scope(profile, identity),
        entity: identity.entity.clone(),
        field: field.into(),
    }
}

pub(super) fn values(model: &Model) -> BTreeMap<Key, Value> {
    let mut values = BTreeMap::new();
    for tab in model.tabs.values() {
        for identity in &tab.identities {
            for (field, value) in FIELDS.into_iter().zip([
                json!(tab.space_id),
                json!(tab.manual_assignment),
                json!(tab.pinned),
                json!(tab.folder_id),
            ]) {
                values.insert(key(&model.profile, identity, field), value);
            }
        }
    }
    values
}

pub(super) fn restore(
    model: &mut Model,
    values: &BTreeMap<Key, Value>,
    only: Option<&BTreeSet<TabId>>,
) -> Result<bool, Error> {
    let before = model.revision;
    let selected = model.selected_tab;
    let selected_space = model.selected_space.clone();
    for id in model.tab_order().to_vec() {
        if only.is_some_and(|ids| !ids.contains(&id)) {
            continue;
        }
        let tab = &model.tabs[&id];
        let Some(identity) = tab.identities.iter().find(|identity| {
            FIELDS
                .iter()
                .any(|field| values.contains_key(&key(&model.profile, identity, field)))
        }) else {
            continue;
        };
        let read = |field: &str, fallback: Value| -> Value {
            values
                .get(&key(&model.profile, identity, field))
                .cloned()
                .unwrap_or(fallback)
        };
        let decode = |error| Error(format!("Stored tab placement: {error}"));
        let mut space: String =
            serde_json::from_value(read("space", json!(tab.space_id))).map_err(decode)?;
        let mut manual: bool =
            serde_json::from_value(read("manual", json!(tab.manual_assignment))).map_err(decode)?;
        let pinned: bool =
            serde_json::from_value(read("pinned", json!(tab.pinned))).map_err(decode)?;
        let folder: Option<String> =
            serde_json::from_value(read("folder", json!(tab.folder_id))).map_err(decode)?;
        // Deleted spaces and folders are never recreated by a returning shell.
        if !model.spaces.iter().any(|existing| existing.id == space) {
            space = tab.space_id.clone();
            manual = false;
        }
        model.restore_tab_membership(id, &space, manual, pinned, folder.as_deref())?;
    }
    if let Some(id) = selected
        && model.tabs[&id].space_id != selected_space
    {
        model.dispatch(Intent::ActivateTab(id))?;
    }
    Ok(model.revision != before)
}
