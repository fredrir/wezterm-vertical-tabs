use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// A segmented option path. Dynamic keys are kept as opaque segments, so `A.B` inside an open map
/// is not mistaken for two nested keys.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SettingPath(pub Vec<String>);

impl SettingPath {
    pub fn from_dotted(key: &str) -> Self {
        Self(key.split('.').map(str::to_owned).collect())
    }

    pub fn child(&self, key: &str) -> Self {
        let mut parts = self.0.clone();
        parts.push(key.to_owned());
        Self(parts)
    }

    pub fn dotted(&self) -> String {
        self.0.join(".")
    }

    pub fn starts_with(&self, other: &Self) -> bool {
        self.0.starts_with(&other.0)
    }
}

/// The serializable part of a plugin option. Lua functions deliberately have no `Value`: they are
/// config-as-code and never belong in the settings file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Value {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    List(Vec<Value>),
    Table(BTreeMap<String, Value>),
}

impl Value {
    pub fn as_table(&self) -> Option<&BTreeMap<String, Value>> {
        match self {
            Self::Table(table) => Some(table),
            _ => None,
        }
    }

    pub fn as_table_mut(&mut self) -> Option<&mut BTreeMap<String, Value>> {
        match self {
            Self::Table(table) => Some(table),
            _ => None,
        }
    }
}

impl From<bool> for Value {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

macro_rules! number_from {
    ($($kind:ty),+ $(,)?) => {
        $(
            impl From<$kind> for Value {
                fn from(value: $kind) -> Self {
                    Self::Number(value as f64)
                }
            }
        )+
    };
}

number_from!(i32, i64, u32, u64, usize, f32);

impl From<f64> for Value {
    fn from(value: f64) -> Self {
        Self::Number(value)
    }
}

impl From<&str> for Value {
    fn from(value: &str) -> Self {
        Self::String(value.to_owned())
    }
}

impl From<String> for Value {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<Vec<Value>> for Value {
    fn from(value: Vec<Value>) -> Self {
        Self::List(value)
    }
}

impl From<BTreeMap<String, Value>> for Value {
    fn from(value: BTreeMap<String, Value>) -> Self {
        Self::Table(value)
    }
}

pub fn table<const N: usize>(entries: [(&str, Value); N]) -> Value {
    Value::Table(
        entries
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
    )
}

pub fn get_path<'a>(root: &'a Value, key: &str) -> Option<&'a Value> {
    get_at(root, &SettingPath::from_dotted(key))
}

pub fn get_at<'a>(root: &'a Value, path: &SettingPath) -> Option<&'a Value> {
    let mut node = root;
    for part in &path.0 {
        node = node.as_table()?.get(part)?;
    }
    Some(node)
}

pub fn set_path(root: &mut Value, key: &str, value: Value) {
    set_at(root, &SettingPath::from_dotted(key), Some(value));
}

pub fn set_at(root: &mut Value, path: &SettingPath, value: Option<Value>) {
    let mut parts = path.0.iter().peekable();
    let mut node = root;
    while let Some(part) = parts.next() {
        if parts.peek().is_none() {
            let table = node
                .as_table_mut()
                .expect("the schema root and every built parent are tables");
            if let Some(value) = value {
                table.insert(part.clone(), value);
            } else {
                table.remove(part);
            }
            return;
        }
        let table = node
            .as_table_mut()
            .expect("the schema root and every built parent are tables");
        node = table
            .entry(part.clone())
            .or_insert_with(|| Value::Table(BTreeMap::new()));
    }
}

#[cfg(test)]
#[path = "../../tests/unit/settings/value.rs"]
mod tests;
