//! Stateless space routing and topology planning.
//!
//! WezTerm observation, Lua callback execution, persistence and mux mutation deliberately stay
//! outside this module. Every piece of memory that affects a decision enters in [`SpacesMsg`] and
//! leaves in [`SpaceResolution`], so the same planner can serve any future host adapter.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value};
use vtabs_protocol::payload::{
    DynamicSpace, Scheme, SpaceAssignment, SpaceFollow, SpaceLastTab, SpaceResolution,
    SpaceRouteHookAnswer, SpaceRouteHookFact, SpaceSummary, SpaceTabFact, SpaceWarning, SpacesMsg,
    ThemeOverrides,
};

use crate::{enrich::title_of, sanitize, theme};

const ID_MAX_BYTES: usize = 48;
const MAX_SPACES: usize = vtabs_protocol::limits::MODEL_MAX_SPACES;
/// Lua numbers and the deterministic wire encoder preserve every integer through this value.
const LUA_MAX_EXACT_INT: u64 = 9_007_199_254_740_991;
const LAST_DYNAMIC_SEQ: u64 = LUA_MAX_EXACT_INT - 1;
const ACCENT_SLOTS: [usize; 6] = [5, 3, 4, 6, 7, 2];

#[derive(Debug, Clone, PartialEq)]
pub enum Plan {
    NeedsHooks { tabs: Vec<SpaceRouteHookFact> },
    Resolved(Box<SpaceResolution>),
}

#[derive(Debug, Clone, PartialEq)]
enum DefinitionTheme {
    Auto,
    Overrides(Box<ThemeOverrides>),
}

#[derive(Debug, Clone, PartialEq)]
enum Patterns {
    One(String),
    Any(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Default)]
struct Rule {
    remote: Option<bool>,
    fields: BTreeMap<String, Patterns>,
}

#[derive(Debug, Clone, PartialEq)]
struct Definition {
    id: String,
    name: Option<String>,
    icon: Option<String>,
    theme: Option<DefinitionTheme>,
    rule: Option<Rule>,
}

fn warning(code: &str, space_id: Option<&str>, field: Option<&str>) -> SpaceWarning {
    SpaceWarning {
        code: code.to_owned(),
        space_id: space_id.map(str::to_owned),
        field: field.map(str::to_owned),
    }
}

fn string_part(
    object: &Map<String, Value>,
    key: &str,
    id: &str,
    warnings: &mut Vec<SpaceWarning>,
) -> Option<String> {
    match object.get(key) {
        None | Some(Value::Null) => None,
        Some(Value::String(value)) => Some(value.clone()),
        Some(_) => {
            warnings.push(warning("invalid_definition_field", Some(id), Some(key)));
            None
        }
    }
}

fn color(value: &Value) -> Option<String> {
    if let Some(value) = value.as_str() {
        return theme::parse(value).map(|_| value.to_owned());
    }
    let values = value.as_array()?;
    if values.len() != 3 {
        return None;
    }
    let mut rgb = [0_u8; 3];
    for (at, value) in values.iter().enumerate() {
        rgb[at] = u8::try_from(value.as_u64()?).ok()?;
    }
    Some(format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2]))
}

fn theme_overrides(
    object: &Map<String, Value>,
    id: &str,
    warnings: &mut Vec<SpaceWarning>,
) -> ThemeOverrides {
    let mut out = ThemeOverrides::default();
    macro_rules! colors {
        ($($field:ident),+ $(,)?) => {$({
            let key = stringify!($field);
            if let Some(value) = object.get(key) {
                if value.is_null() {
                    // A nil Lua field is absent on the wire; tolerate JSON null the same way.
                } else if let Some(value) = color(value) {
                    out.$field = Some(value);
                } else {
                    warnings.push(warning("invalid_theme_field", Some(id), Some(key)));
                }
            }
        })+};
    }
    crate::theme::theme_color_fields!(colors);
    for key in object
        .keys()
        .filter(|key| !theme::is_override_field(key.as_str()))
    {
        warnings.push(warning("unknown_theme_field", Some(id), Some(key)));
    }
    macro_rules! fractions {
        ($($field:ident),+ $(,)?) => {$({
            let key = stringify!($field);
            if let Some(value) = object.get(key) {
                if value.is_null() {
                    // A nil Lua field is absent on the wire; tolerate JSON null the same way.
                } else if let Some(value) = value
                    .as_f64()
                    .filter(|value| value.is_finite() && (0.0..=1.0).contains(value))
                {
                    out.$field = Some(value);
                } else {
                    warnings.push(warning("invalid_theme_field", Some(id), Some(key)));
                }
            }
        })+};
    }
    crate::theme::theme_fraction_fields!(fractions);
    out
}

fn definition_theme(
    value: Option<&Value>,
    id: &str,
    warnings: &mut Vec<SpaceWarning>,
) -> Option<DefinitionTheme> {
    match value {
        None | Some(Value::Null) => None,
        Some(Value::String(value)) if value == "auto" => Some(DefinitionTheme::Auto),
        Some(Value::Object(object)) => Some(DefinitionTheme::Overrides(Box::new(theme_overrides(
            object, id, warnings,
        )))),
        Some(_) => {
            warnings.push(warning("invalid_theme", Some(id), Some("theme")));
            None
        }
    }
}

fn patterns(value: &Value, id: &str, field: &str, warnings: &mut Vec<SpaceWarning>) -> Patterns {
    match value {
        Value::String(value) => Patterns::One(value.clone()),
        Value::Array(values) => {
            let strings = values
                .iter()
                .filter_map(|value| value.as_str().map(str::to_owned))
                .collect::<Vec<_>>();
            if strings.len() != values.len() {
                warnings.push(warning("invalid_match_field", Some(id), Some(field)));
            }
            Patterns::Any(strings)
        }
        _ => {
            warnings.push(warning("invalid_match_field", Some(id), Some(field)));
            Patterns::Any(Vec::new())
        }
    }
}

fn definition_rule(
    value: Option<&Value>,
    id: &str,
    warnings: &mut Vec<SpaceWarning>,
) -> Option<Rule> {
    let value = value?;
    let Some(object) = value.as_object() else {
        warnings.push(warning("invalid_match", Some(id), Some("match")));
        return None;
    };
    let mut out = Rule::default();
    for (field, value) in object {
        match field.as_str() {
            "remote" => {
                if let Some(value) = value.as_bool() {
                    out.remote = Some(value);
                } else {
                    warnings.push(warning("invalid_match_field", Some(id), Some(field)));
                }
            }
            "domain" | "host" | "user" | "proc" | "cwd" | "title" => {
                out.fields
                    .insert(field.clone(), patterns(value, id, field, warnings));
            }
            _ => warnings.push(warning("unknown_match_field", Some(id), Some(field))),
        }
    }
    Some(out)
}

fn definitions(msg: &SpacesMsg) -> (Vec<Definition>, Vec<SpaceWarning>) {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    let mut warnings = Vec::new();
    for value in &msg.definitions {
        let Some(object) = value.as_object() else {
            warnings.push(warning("missing_id", None, Some("id")));
            continue;
        };
        let Some(id) = object
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
        else {
            warnings.push(warning("missing_id", None, Some("id")));
            continue;
        };
        if !seen.insert(id.to_owned()) {
            warnings.push(warning("duplicate_id", Some(id), Some("id")));
            continue;
        }
        if out.len() >= MAX_SPACES {
            warnings.push(warning("too_many_definitions", Some(id), None));
            continue;
        }
        out.push(Definition {
            id: id.to_owned(),
            name: string_part(object, "name", id, &mut warnings),
            icon: string_part(object, "icon", id, &mut warnings),
            theme: definition_theme(object.get("theme"), id, &mut warnings),
            rule: definition_rule(object.get("match"), id, &mut warnings),
        });
    }
    if out.is_empty() && msg.enabled && msg.hook {
        out.push(Definition {
            id: "home".to_owned(),
            name: Some("Home".to_owned()),
            icon: None,
            theme: None,
            rule: None,
        });
    }
    (out, warnings)
}

fn template(id: &str) -> bool {
    id.contains('$')
}

fn truncate_bytes(value: &str, max: usize) -> &str {
    if value.len() <= max {
        return value;
    }
    let mut end = max;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

fn fact<'a>(tab: &'a SpaceTabFact, name: &str) -> Option<std::borrow::Cow<'a, str>> {
    match name {
        "domain" => tab.tab.domain.as_deref().map(std::borrow::Cow::Borrowed),
        "host" => tab.tab.host.as_deref().map(std::borrow::Cow::Borrowed),
        "user" => tab.tab.user.as_deref().map(std::borrow::Cow::Borrowed),
        "proc" => tab.tab.proc.as_deref().map(std::borrow::Cow::Borrowed),
        "cwd" => tab.tab.cwd.as_deref().map(std::borrow::Cow::Borrowed),
        "title" => Some(std::borrow::Cow::Owned(title_of(&tab.tab))),
        _ => None,
    }
    .filter(|value| !value.is_empty())
}

fn expand(value: &str, tab: &SpaceTabFact) -> Option<String> {
    if !template(value) {
        return Some(value.to_owned());
    }
    let mut out = String::new();
    let chars = value.char_indices().collect::<Vec<_>>();
    let mut at = 0;
    while at < chars.len() {
        let (byte, ch) = chars[at];
        if ch != '$' {
            let next = chars.get(at + 1).map_or(value.len(), |(byte, _)| *byte);
            out.push_str(&value[byte..next]);
            at += 1;
            continue;
        }
        let mut end = at + 1;
        while end < chars.len() && chars[end].1.is_ascii_alphabetic() {
            end += 1;
        }
        if end == at + 1 {
            out.push('$');
            at += 1;
            continue;
        }
        let start_byte = chars[at + 1].0;
        let end_byte = chars.get(end).map_or(value.len(), |(byte, _)| *byte);
        let replacement = fact(tab, &value[start_byte..end_byte])?;
        out.push_str(&replacement);
        at = end;
    }
    let out = sanitize(truncate_bytes(&out, ID_MAX_BYTES).as_bytes());
    (!out.is_empty()).then_some(out)
}

fn glob_matches(pattern: &str, value: &str) -> bool {
    // Classic single-star backtracking: only `*` is special and the match remains anchored at
    // both ends. Greedy first-find is insufficient (`*a` must match the final `a` in `aba`).
    let (pattern, value) = (pattern.as_bytes(), value.as_bytes());
    let (mut p, mut v, mut star, mut retry) = (0, 0, None, 0);
    while v < value.len() {
        if p < pattern.len() && pattern[p] != b'*' && pattern[p] == value[v] {
            p += 1;
            v += 1;
        } else if p < pattern.len() && pattern[p] == b'*' {
            star = Some(p);
            p += 1;
            retry = v;
        } else if let Some(at) = star {
            retry += 1;
            v = retry;
            p = at + 1;
        } else {
            return false;
        }
    }
    while p < pattern.len() && pattern[p] == b'*' {
        p += 1;
    }
    p == pattern.len()
}

fn pattern_matches(field: &str, pattern: &str, value: Option<&str>) -> bool {
    let Some(value) = value else {
        return false;
    };
    if field == "cwd" && !pattern.contains('*') {
        return value == pattern
            || value
                .strip_prefix(pattern)
                .is_some_and(|rest| rest.starts_with('/'));
    }
    glob_matches(pattern, value)
}

fn matches(rule: &Rule, tab: &SpaceTabFact) -> bool {
    if rule.remote.is_some_and(|remote| remote != tab.remote) {
        return false;
    }
    rule.fields.iter().all(|(field, patterns)| match patterns {
        Patterns::One(pattern) => pattern_matches(field, pattern, fact(tab, field).as_deref()),
        Patterns::Any(patterns) => patterns
            .iter()
            .any(|pattern| pattern_matches(field, pattern, fact(tab, field).as_deref())),
    })
}

fn route<'a>(
    definitions: &'a [Definition],
    tab: &SpaceTabFact,
) -> Option<(String, &'a Definition)> {
    definitions.iter().find_map(|definition| {
        definition
            .rule
            .as_ref()
            .filter(|rule| matches(rule, tab))
            .and_then(|_| expand(&definition.id, tab))
            .map(|id| (id, definition))
    })
}

fn fingerprint(msg: &SpacesMsg, tab: &SpaceTabFact, space: Option<&str>, manual: bool) -> String {
    let mut hash = 2_166_136_261_u32;
    let mut push = |value: Option<&str>| {
        for byte in value.unwrap_or("").bytes().chain(std::iter::once(0)) {
            hash ^= u32::from(byte);
            hash = hash.wrapping_mul(16_777_619);
        }
    };
    push(Some(&tab.tab.id.to_string()));
    push(Some(&msg.window_id.to_string()));
    let title = title_of(&tab.tab);
    push(Some(&title));
    push(tab.tab.proc.as_deref());
    push(tab.tab.cwd.as_deref());
    push(tab.tab.host.as_deref());
    push(tab.tab.user.as_deref());
    push(tab.tab.domain.as_deref());
    push(tab.remote.then_some("1"));
    push(space);
    push(manual.then_some("1"));
    format!("{hash:08x}")
}

fn hook_fact(msg: &SpacesMsg, tab: &SpaceTabFact) -> SpaceRouteHookFact {
    SpaceRouteHookFact {
        tab_id: tab.tab.id,
        window_id: msg.window_id,
        title: title_of(&tab.tab),
        proc: tab.tab.proc.clone(),
        cwd: tab.tab.cwd.clone(),
        host: tab.tab.host.clone(),
        user: tab.tab.user.clone(),
        domain: tab.tab.domain.clone(),
        remote: tab.remote,
        space: tab.space.clone(),
        manual: tab.manual,
    }
}

fn answer_map(answers: Option<&[SpaceRouteHookAnswer]>) -> BTreeMap<i64, Option<String>> {
    answers
        .unwrap_or_default()
        .iter()
        .map(|answer| (answer.tab_id, answer.space.clone()))
        .collect()
}

fn clean_hook_space(value: Option<&str>) -> Option<String> {
    let value = value?;
    let value = sanitize(truncate_bytes(value, ID_MAX_BYTES).as_bytes());
    (!value.is_empty()).then_some(value)
}

fn definition_for<'a>(
    definitions: &'a [Definition],
    dynamics: &BTreeMap<String, DynamicSpace>,
    id: &str,
) -> Option<&'a Definition> {
    definitions
        .iter()
        .find(|definition| definition.id == id)
        .or_else(|| {
            let template = dynamics.get(id)?.template.as_deref()?;
            definitions
                .iter()
                .find(|definition| definition.id == template)
        })
}

fn auto_accent(id: &str, scheme: &Scheme) -> Option<String> {
    let mut slots = Vec::new();
    for source in [&scheme.ansi, &scheme.brights] {
        for one_based in ACCENT_SLOTS {
            if let Some(color) = source.get(one_based - 1) {
                slots.push(color);
            }
        }
    }
    if slots.is_empty() {
        return None;
    }
    let mut hash = 2_166_136_261_u32;
    for byte in id.bytes() {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(16_777_619);
    }
    Some(slots[hash as usize % slots.len()].clone())
}

fn theme_layer(
    active: Option<&str>,
    definitions: &[Definition],
    dynamics: &BTreeMap<String, DynamicSpace>,
    scheme: &Scheme,
) -> ThemeOverrides {
    let Some(id) = active else {
        return ThemeOverrides::default();
    };
    let entry = definition_for(definitions, dynamics, id);
    if let Some(DefinitionTheme::Overrides(overrides)) =
        entry.and_then(|entry| entry.theme.as_ref())
    {
        return overrides.as_ref().clone();
    }
    let automatic = match entry {
        None => true,
        Some(entry) => matches!(entry.theme, Some(DefinitionTheme::Auto)) || template(&entry.id),
    };
    let mut out = ThemeOverrides::default();
    if automatic {
        out.accent = auto_accent(id, scheme);
    }
    out
}

/// Validate and plan a complete window. `answers == None` is the first pass. When a configured
/// route hook is needed, the function returns every cache miss as one batch; a second call with one
/// explicit answer per requested tab produces the final resolution.
pub fn plan(msg: &SpacesMsg, scheme: &Scheme, answers: Option<&[SpaceRouteHookAnswer]>) -> Plan {
    let (definitions, mut warnings) = definitions(msg);
    let default = definitions.first().map(|definition| definition.id.clone());
    let fingerprints = msg
        .tabs
        .iter()
        .map(|tab| {
            (
                tab.tab.id,
                fingerprint(msg, tab, tab.space.as_deref(), tab.manual),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let answers = answer_map(answers);

    if msg.enabled && msg.hook {
        let needed = msg
            .tabs
            .iter()
            .filter(|tab| !tab.tab.settings)
            .filter(|tab| !(tab.space.is_some() && tab.manual))
            .filter(|tab| {
                !(tab.space.is_some() && tab.fingerprint.as_ref() == fingerprints.get(&tab.tab.id))
            })
            .filter(|tab| !answers.contains_key(&tab.tab.id))
            .map(|tab| hook_fact(msg, tab))
            .collect::<Vec<_>>();
        if !needed.is_empty() {
            return Plan::NeedsHooks { tabs: needed };
        }
    }

    if !msg.enabled {
        return Plan::Resolved(Box::new(SpaceResolution {
            active: None,
            assignments: msg
                .tabs
                .iter()
                .map(|tab| SpaceAssignment {
                    tab_id: tab.tab.id,
                    space: tab.space.clone(),
                    manual: tab.manual,
                    fingerprint: tab.fingerprint.clone(),
                })
                .collect(),
            // Disabling spaces is authoritative too: no dynamic namespace survives invisibly and
            // then reappears with stale first-sight order when the feature is enabled again.
            dynamics: Vec::new(),
            follow: msg.follow.clone(),
            last_tabs: msg.last_tabs.clone(),
            summary: Vec::new(),
            visible_tab_ids: msg.tabs.iter().map(|tab| tab.tab.id).collect(),
            theme_overrides: ThemeOverrides::default(),
            warnings,
        }));
    }

    let static_ids = definitions
        .iter()
        .filter(|definition| !template(&definition.id))
        .map(|definition| definition.id.clone())
        .collect::<BTreeSet<_>>();
    let mut present = static_ids.clone();
    let mut dynamics = msg
        .dynamics
        .iter()
        .cloned()
        .map(|mut space| {
            space.seq = space.seq.min(LUA_MAX_EXACT_INT);
            (space.id.clone(), space)
        })
        .collect::<BTreeMap<_, _>>();
    let mut next_seq = dynamics
        .values()
        .filter(|space| space.seq != LUA_MAX_EXACT_INT)
        .map(|space| space.seq)
        .max()
        .unwrap_or(0)
        .saturating_add(1)
        .min(LAST_DYNAMIC_SEQ);
    let mut assignments = Vec::with_capacity(msg.tabs.len());
    let mut assigned = BTreeMap::new();

    for tab in &msg.tabs {
        if tab.tab.settings {
            assignments.push(SpaceAssignment {
                tab_id: tab.tab.id,
                space: None,
                manual: false,
                fingerprint: None,
            });
            assigned.insert(tab.tab.id, None);
            continue;
        }
        let incoming_stamp = fingerprints.get(&tab.tab.id);
        let mut space = tab.space.clone();
        let mut manual = tab.manual && space.is_some();
        let unchanged = space.is_some() && tab.fingerprint.as_ref() == incoming_stamp;
        if !manual && !unchanged {
            let hook_target = msg
                .hook
                .then(|| answers.get(&tab.tab.id))
                .flatten()
                .and_then(|space| clean_hook_space(space.as_deref()));
            let routed = hook_target
                .map(|space| (space, None))
                .or_else(|| route(&definitions, tab).map(|(space, entry)| (space, Some(entry))));
            let target = routed.and_then(|(target, entry)| {
                if !present.contains(&target) && present.len() >= MAX_SPACES {
                    warnings.push(warning("spaces_max", Some(&target), None));
                    return None;
                }
                if !static_ids.contains(&target) && !dynamics.contains_key(&target) {
                    let name = entry
                        .and_then(|entry| expand(entry.name.as_deref().unwrap_or(&entry.id), tab))
                        .unwrap_or_else(|| target.clone());
                    dynamics.insert(
                        target.clone(),
                        DynamicSpace {
                            id: target.clone(),
                            name,
                            template: entry.map(|entry| entry.id.clone()),
                            seq: next_seq,
                        },
                    );
                    next_seq = next_seq.saturating_add(1).min(LAST_DYNAMIC_SEQ);
                }
                Some(target)
            });
            if let Some(target) = target {
                space = Some(target);
                manual = false;
            } else if space.is_none() {
                space = msg.active_space.clone().or_else(|| default.clone());
            }
        }
        if let Some(space) = &space {
            present.insert(space.clone());
        }
        assigned.insert(tab.tab.id, space.clone());
        let stamp = fingerprint(msg, tab, space.as_deref(), manual);
        assignments.push(SpaceAssignment {
            tab_id: tab.tab.id,
            space,
            manual,
            fingerprint: Some(stamp),
        });
    }

    let mut active = msg.active_space.clone();
    let mut follow = msg.follow.clone();
    let mut last_tabs = msg
        .last_tabs
        .iter()
        .map(|entry| (entry.space_id.clone(), entry.tab_id))
        .collect::<BTreeMap<_, _>>();
    if let Some(tab_id) = msg.active_tab {
        let tab_space = assigned.get(&tab_id).cloned().flatten();
        if let Some(space) = &tab_space {
            last_tabs.insert(space.clone(), tab_id);
        }
        let observed = SpaceFollow {
            tab_id,
            space: tab_space.clone(),
        };
        if follow.as_ref() != Some(&observed) {
            follow = Some(observed);
            if tab_space.is_some() {
                active = tab_space;
            }
        }
    }
    if active
        .as_ref()
        .is_none_or(|active| !present.contains(active))
    {
        active = default;
    }

    let mut counts = BTreeMap::<String, usize>::new();
    let mut unseen = BTreeSet::new();
    for tab in &msg.tabs {
        if tab.tab.settings {
            continue;
        }
        if let Some(space) = assigned.get(&tab.tab.id).cloned().flatten() {
            *counts.entry(space.clone()).or_default() += 1;
            if tab.tab.unseen {
                unseen.insert(space);
            }
        }
    }
    for id in counts.keys() {
        if !static_ids.contains(id) && !dynamics.contains_key(id) {
            dynamics.insert(
                id.clone(),
                DynamicSpace {
                    id: id.clone(),
                    name: id.clone(),
                    template: None,
                    seq: LUA_MAX_EXACT_INT,
                },
            );
        }
    }

    let mut summary = definitions
        .iter()
        .filter(|definition| !template(&definition.id))
        .map(|definition| SpaceSummary {
            id: definition.id.clone(),
            name: definition
                .name
                .clone()
                .unwrap_or_else(|| definition.id.clone()),
            icon: definition.icon.clone(),
            count: counts.get(&definition.id).copied().unwrap_or(0),
            unseen: unseen.contains(&definition.id),
        })
        .collect::<Vec<_>>();
    let seen = summary
        .iter()
        .map(|space| space.id.clone())
        .collect::<BTreeSet<_>>();
    let mut dynamic_ids = counts
        .keys()
        .filter(|id| !seen.contains(*id))
        .cloned()
        .collect::<Vec<_>>();
    dynamic_ids.sort_by(|a, b| dynamics[a].seq.cmp(&dynamics[b].seq).then_with(|| a.cmp(b)));
    for id in dynamic_ids {
        let meta = &dynamics[&id];
        let icon =
            definition_for(&definitions, &dynamics, &id).and_then(|entry| entry.icon.clone());
        summary.push(SpaceSummary {
            id: id.clone(),
            name: meta.name.clone(),
            icon,
            count: counts.get(&id).copied().unwrap_or(0),
            unseen: unseen.contains(&id),
        });
    }

    let visible_tab_ids = msg
        .tabs
        .iter()
        .filter(|tab| {
            tab.tab.settings
                || assigned
                    .get(&tab.tab.id)
                    .is_none_or(|space| space.is_none() || space == &active)
        })
        .map(|tab| tab.tab.id)
        .collect();
    let theme_overrides = theme_layer(active.as_deref(), &definitions, &dynamics, scheme);
    // The returned ledger is authoritative for this window. Empty dynamic spaces disappear; if an
    // id returns later it is admitted again at the end of that window's first-sight order.
    dynamics.retain(|id, _| counts.contains_key(id));
    let mut dynamics = dynamics.into_values().collect::<Vec<_>>();
    dynamics.sort_by(|a, b| a.seq.cmp(&b.seq).then_with(|| a.id.cmp(&b.id)));

    Plan::Resolved(Box::new(SpaceResolution {
        active,
        assignments,
        dynamics,
        follow,
        last_tabs: last_tabs
            .into_iter()
            .map(|(space_id, tab_id)| SpaceLastTab { space_id, tab_id })
            .collect(),
        summary,
        visible_tab_ids,
        theme_overrides,
        warnings,
    }))
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use vtabs_protocol::payload::{Scheme, SpaceRouteHookAnswer, SpacesMsg};

    use super::{Plan, fingerprint, glob_matches, plan};

    fn msg(value: serde_json::Value) -> SpacesMsg {
        serde_json::from_value(value).unwrap()
    }

    fn scheme() -> Scheme {
        serde_json::from_value(json!({
            "ansi": ["#000000", "#110000", "#220000", "#330000", "#440000", "#550000", "#660000"],
            "brights": ["#777777", "#880000", "#990000", "#aa0000", "#bb0000", "#cc0000", "#dd0000"]
        }))
        .unwrap()
    }

    fn resolved(
        msg: &SpacesMsg,
        answers: Option<&[SpaceRouteHookAnswer]>,
    ) -> vtabs_protocol::payload::SpaceResolution {
        match plan(msg, &scheme(), answers) {
            Plan::Resolved(result) => *result,
            Plan::NeedsHooks { .. } => panic!("unexpected hook request"),
        }
    }

    #[test]
    fn rules_are_anchored_lists_are_any_of_and_cwd_without_star_is_a_prefix() {
        assert!(glob_matches("*a", "aba"));
        assert!(glob_matches("a*a", "aba"));
        assert!(!glob_matches("a*b", "abx"));
        let msg = msg(json!({
            "window_id": 7, "enabled": true,
            "definitions": [
                {"id":"work", "match":{"domain":["tls:*","ssh:*"], "cwd":"~/work"}},
                {"id":"claude", "match":{"proc":"cla*"}},
                {"id":"home"}
            ],
            "tabs": [
                {"id":1,"index":1,"title":"a","domain":"ssh:pi","cwd":"~/work/x","proc":"zsh"},
                {"id":2,"index":2,"title":"b","domain":"local","cwd":"~/workshop","proc":"xclaude"},
                {"id":3,"index":3,"title":"c","domain":"local","cwd":"~","proc":"claude"}
            ], "active_tab": 1
        }));
        let result = resolved(&msg, None);
        assert_eq!(result.assignments[0].space.as_deref(), Some("work"));
        assert_eq!(
            result.assignments[1].space.as_deref(),
            Some("work"),
            "falls into active/default after both anchored rules miss"
        );
        assert_eq!(result.assignments[2].space.as_deref(), Some("claude"));
    }

    #[test]
    fn templates_are_utf8_safe_capped_and_skip_missing_facts() {
        let msg = msg(json!({
            "window_id":1,"enabled":true,
            "definitions":[
                {"id":"home"},
                {"id":"$host", "name":"Remote $host", "match":{"remote":true}}
            ],
            "tabs":[
                {"id":1,"index":1,"title":"one","remote":true,"host":"éééééééééééééééééééééééééééééé"},
                {"id":2,"index":2,"title":"two","remote":true}
            ], "active_tab":1
        }));
        let result = resolved(&msg, None);
        let id = result.assignments[0].space.as_ref().unwrap();
        assert!(id.len() <= 48);
        assert!(id.is_char_boundary(id.len()));
        assert_eq!(result.assignments[1].space.as_deref(), Some("home"));
    }

    #[test]
    fn hook_is_batched_only_for_changed_automatic_tabs_and_wins_before_rules() {
        let msg = msg(json!({
            "window_id":9,"enabled":true,"hook":true,
            "definitions":[{"id":"home"},{"id":"rules","match":{"proc":"cargo"}}],
            "tabs":[
                {"id":1,"index":1,"title":"one","proc":"zsh","space":"home","fingerprint":"stale"},
                {"id":2,"index":2,"title":"two","proc":"cargo","space":"home","manual":true},
                {"id":3,"index":3,"title":"three","proc":"cargo"}
            ], "active_tab":1
        }));
        let requested = match plan(&msg, &scheme(), None) {
            Plan::NeedsHooks { tabs } => tabs,
            Plan::Resolved(_) => panic!("hook batch expected"),
        };
        assert_eq!(
            requested.iter().map(|tab| tab.tab_id).collect::<Vec<_>>(),
            [1, 3]
        );
        let answers = [
            SpaceRouteHookAnswer {
                tab_id: 1,
                space: Some("hooked".into()),
            },
            SpaceRouteHookAnswer {
                tab_id: 3,
                space: None,
            },
        ];
        let result = resolved(&msg, Some(&answers));
        assert_eq!(result.assignments[0].space.as_deref(), Some("hooked"));
        assert_eq!(result.assignments[1].space.as_deref(), Some("home"));
        assert_eq!(result.assignments[2].space.as_deref(), Some("rules"));

        let mut next = msg;
        for (tab, assignment) in next.tabs.iter_mut().zip(&result.assignments) {
            tab.space.clone_from(&assignment.space);
            tab.fingerprint.clone_from(&assignment.fingerprint);
        }
        assert!(matches!(plan(&next, &scheme(), None), Plan::Resolved(_)));
    }

    #[test]
    fn hook_fingerprint_covers_every_fact_the_callback_can_observe() {
        let base = msg(json!({
            "window_id":9,"enabled":true,"hook":true,
            "tabs":[{"id":1,"index":1,"title":"one","proc":"zsh","cwd":"~","host":"host",
                "user":"user","domain":"local","remote":false,"space":"home","manual":false}]
        }));
        let stamp = fingerprint(&base, &base.tabs[0], Some("home"), false);
        let changed = |value: serde_json::Value| {
            let changed = msg(value);
            fingerprint(
                &changed,
                &changed.tabs[0],
                changed.tabs[0].space.as_deref(),
                changed.tabs[0].manual,
            )
        };
        for value in [
            json!({"window_id":10,"enabled":true,"hook":true,"tabs":[{"id":1,"index":1,"title":"one","proc":"zsh","cwd":"~","host":"host","user":"user","domain":"local","remote":false,"space":"home","manual":false}]}),
            json!({"window_id":9,"enabled":true,"hook":true,"tabs":[{"id":2,"index":1,"title":"one","proc":"zsh","cwd":"~","host":"host","user":"user","domain":"local","remote":false,"space":"home","manual":false}]}),
            json!({"window_id":9,"enabled":true,"hook":true,"tabs":[{"id":1,"index":1,"title":"two","proc":"zsh","cwd":"~","host":"host","user":"user","domain":"local","remote":false,"space":"home","manual":false}]}),
            json!({"window_id":9,"enabled":true,"hook":true,"tabs":[{"id":1,"index":1,"title":"one","proc":"fish","cwd":"~","host":"host","user":"user","domain":"local","remote":false,"space":"home","manual":false}]}),
            json!({"window_id":9,"enabled":true,"hook":true,"tabs":[{"id":1,"index":1,"title":"one","proc":"zsh","cwd":"/tmp","host":"host","user":"user","domain":"local","remote":false,"space":"home","manual":false}]}),
            json!({"window_id":9,"enabled":true,"hook":true,"tabs":[{"id":1,"index":1,"title":"one","proc":"zsh","cwd":"~","host":"other","user":"user","domain":"local","remote":false,"space":"home","manual":false}]}),
            json!({"window_id":9,"enabled":true,"hook":true,"tabs":[{"id":1,"index":1,"title":"one","proc":"zsh","cwd":"~","host":"host","user":"other","domain":"local","remote":false,"space":"home","manual":false}]}),
            json!({"window_id":9,"enabled":true,"hook":true,"tabs":[{"id":1,"index":1,"title":"one","proc":"zsh","cwd":"~","host":"host","user":"user","domain":"ssh","remote":false,"space":"home","manual":false}]}),
            json!({"window_id":9,"enabled":true,"hook":true,"tabs":[{"id":1,"index":1,"title":"one","proc":"zsh","cwd":"~","host":"host","user":"user","domain":"local","remote":true,"space":"home","manual":false}]}),
            json!({"window_id":9,"enabled":true,"hook":true,"tabs":[{"id":1,"index":1,"title":"one","proc":"zsh","cwd":"~","host":"host","user":"user","domain":"local","remote":false,"space":"work","manual":false}]}),
            json!({"window_id":9,"enabled":true,"hook":true,"tabs":[{"id":1,"index":1,"title":"one","proc":"zsh","cwd":"~","host":"host","user":"user","domain":"local","remote":false,"space":"home","manual":true}]}),
        ] {
            assert_ne!(changed(value), stamp);
        }
    }

    #[test]
    fn sticky_assignment_holds_on_no_match_but_moves_into_a_new_match() {
        let mut msg = msg(json!({
            "window_id":1,"enabled":true,
            "definitions":[{"id":"home"},{"id":"claude","match":{"proc":"claude"}}],
            "tabs":[{"id":1,"index":1,"title":"one","proc":"zsh","space":"home","fingerprint":"old"}],
            "active_tab":1,"active_space":"home"
        }));
        let first = resolved(&msg, None);
        assert_eq!(first.assignments[0].space.as_deref(), Some("home"));
        msg.tabs[0].tab.proc = Some("claude".into());
        msg.tabs[0]
            .fingerprint
            .clone_from(&first.assignments[0].fingerprint);
        let second = resolved(&msg, None);
        assert_eq!(second.assignments[0].space.as_deref(), Some("claude"));
    }

    #[test]
    fn active_follow_is_a_trigger_and_empty_manual_switch_holds() {
        let base = json!({
            "window_id":1,"enabled":true,
            "definitions":[{"id":"home"},{"id":"scratch"}],
            "tabs":[{"id":1,"index":1,"title":"one","space":"home","fingerprint":"same"}],
            "active_tab":1,"follow":{"tab_id":1,"space":"home"}
        });
        let mut empty = base.clone();
        empty["active_space"] = json!("scratch");
        let held = resolved(&msg(empty), None);
        assert_eq!(held.active.as_deref(), Some("scratch"));

        let mut external = base;
        external["active_space"] = json!("scratch");
        external["follow"] = json!({"tab_id":2,"space":"scratch"});
        let followed = resolved(&msg(external), None);
        assert_eq!(followed.active.as_deref(), Some("home"));
    }

    #[test]
    fn summary_keeps_static_order_then_dynamic_first_sight_and_settings_are_global() {
        let msg = msg(json!({
            "window_id":1,"enabled":true,
            "definitions":[{"id":"home"},{"id":"$host","icon":"R","match":{"remote":true}}],
            "dynamics":[{"id":"old","name":"Old","seq":1},{"id":"empty","name":"Empty","seq":2}],
            "tabs":[
                {"id":1,"index":1,"title":"one","space":"old","fingerprint":"same","unseen":true},
                {"id":2,"index":2,"title":"settings","settings":true},
                {"id":3,"index":3,"title":"three","remote":true,"host":"pi"}
            ], "active_tab":1,"active_space":"old"
        }));
        let result = resolved(&msg, None);
        assert_eq!(
            result
                .summary
                .iter()
                .map(|s| s.id.as_str())
                .collect::<Vec<_>>(),
            ["home", "old", "pi"]
        );
        assert_eq!(result.summary[1].count, 1);
        assert!(result.summary[1].unseen);
        assert_eq!(result.summary[2].icon.as_deref(), Some("R"));
        assert!(result.visible_tab_ids.contains(&2));
        assert!(result.dynamics.iter().all(|space| space.id != "empty"));
    }

    #[test]
    fn disabling_spaces_clears_the_authoritative_dynamic_ledger() {
        let msg = msg(json!({
            "window_id":1,"enabled":false,
            "dynamics":[{"id":"stale","name":"Stale","seq":1}],
            "tabs":[{"id":1,"index":1,"title":"one","space":"stale"}],
            "active_tab":1,"active_space":"stale"
        }));
        let result = resolved(&msg, None);
        assert!(result.dynamics.is_empty());
        assert_eq!(result.active, None);
        assert_eq!(result.visible_tab_ids, vec![1]);
    }

    #[test]
    fn admission_refuses_only_the_dynamic_id_past_the_per_window_cap() {
        let mut definitions = (0..31)
            .map(|at| json!({"id":format!("s{at}")}))
            .collect::<Vec<_>>();
        definitions.push(json!({"id":"$host","match":{"remote":true}}));
        let msg = msg(json!({
            "window_id":44,"enabled":true,"definitions":definitions,
            "tabs":[
                {"id":1,"index":1,"title":"one","remote":true,"host":"pi"},
                {"id":2,"index":2,"title":"two","remote":true,"host":"nas"}
            ],"active_tab":1
        }));
        let result = resolved(&msg, None);
        assert_eq!(result.assignments[0].space.as_deref(), Some("pi"));
        assert_eq!(result.assignments[1].space.as_deref(), Some("s0"));
        assert_eq!(result.summary.len(), 32);
        assert_eq!(
            result
                .warnings
                .iter()
                .filter(|warning| warning.code == "spaces_max")
                .count(),
            1
        );
    }

    #[test]
    fn validation_drops_only_bad_parts_and_theme_layer_is_typed() {
        let msg = msg(json!({
            "window_id":1,"enabled":true,
            "definitions":[
                7,
                {"id":"home","name":9,"theme":{"accent":[1,2,3],"bg":false,"elevation":2,"scrim":-0.1,"mystery":true},"match":{"proc":["zsh",4],"wat":"x"}},
                {"id":"home"},
                {"id":"work","theme":"wrong"},
                {"id":"bad-colour","theme":{"accent":"#zzzzzz"}}
            ],
            "tabs":[{"id":1,"index":1,"title":"one","proc":"zsh"}],"active_tab":1
        }));
        let result = resolved(&msg, None);
        assert_eq!(result.active.as_deref(), Some("home"));
        assert_eq!(result.theme_overrides.accent.as_deref(), Some("#010203"));
        assert_eq!(result.theme_overrides.bg, None);
        assert_eq!(result.theme_overrides.elevation, None);
        assert_eq!(result.theme_overrides.scrim, None);
        let codes = result
            .warnings
            .iter()
            .map(|warning| warning.code.as_str())
            .collect::<Vec<_>>();
        assert!(codes.contains(&"missing_id"));
        assert!(codes.contains(&"invalid_definition_field"));
        assert!(codes.contains(&"invalid_theme_field"));
        assert!(codes.contains(&"unknown_theme_field"));
        assert!(codes.contains(&"invalid_match_field"));
        assert!(codes.contains(&"unknown_match_field"));
        assert!(codes.contains(&"duplicate_id"));
        assert!(codes.contains(&"invalid_theme"));
        assert!(result.warnings.iter().any(|warning| {
            warning.code == "invalid_theme_field"
                && warning.space_id.as_deref() == Some("bad-colour")
                && warning.field.as_deref() == Some("accent")
        }));
    }

    #[test]
    fn automatic_theme_accent_uses_normal_then_bright_palette_slots_stably() {
        let msg = msg(json!({
            "window_id":1,"enabled":true,
            "definitions":[{"id":"home"},{"id":"$host","theme":"auto","match":{"remote":true}}],
            "tabs":[{"id":1,"index":1,"title":"one","remote":true,"host":"pi"}],"active_tab":1
        }));
        let first = resolved(&msg, None).theme_overrides.accent;
        let second = resolved(&msg, None).theme_overrides.accent;
        assert!(first.is_some());
        assert_eq!(first, second);
    }
}
