use serde::Serialize;

use crate::b64;
use crate::types::Mods;

pub use crate::limits::VERSION;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Event {
    Ready {
        v: u8,
        cols: u16,
        rows: u16,
        paints: bool,
    },
    Resize {
        cols: u16,
        rows: u16,
    },
    Key {
        key: String,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        mods: Vec<&'static str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        raw: Option<String>,
    },
    Focus {
        #[serde(rename = "in")]
        focused: bool,
    },
    Paste {
        #[serde(skip_serializing_if = "Option::is_none")]
        data: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        dropped: Option<&'static str>,
    },
    /// `echo` is the ping's own `n`; the monotonic `n` every event carries is added on emit.
    Pong {
        #[serde(skip_serializing_if = "Option::is_none")]
        echo: Option<u64>,
    },
    Dropped {
        what: &'static str,
        reason: &'static str,
    },
    /// The v2 gesture vocabulary; `a` indexes input.lua's `DO` table. The args are boxed: every
    /// verb sets a handful of a wide union, and inline they would size every other event too.
    Do {
        a: &'static str,
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<DoId>,
        #[serde(skip_serializing_if = "DoArgs::is_empty")]
        args: Box<DoArgs>,
    },
    Note {
        k: &'static str,
        #[serde(skip_serializing_if = "Option::is_none")]
        why: Option<&'static str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<i64>,
        /// The level the note is about, so Lua picks its own fallback rather than being told one.
        #[serde(skip_serializing_if = "Option::is_none")]
        a: Option<&'static str>,
    },
    /// What a `kill` or `rescue` did on the server: `detail` is the count moved, or the error.
    Cli {
        op: &'static str,
        ok: bool,
        #[serde(skip_serializing_if = "String::is_empty")]
        detail: String,
    },
}

/// A `do` target: tab ids are numbers, strip buttons are their string id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum DoId {
    Tab(i64),
    Name(String),
}

/// Every argument any `do` action carries; each event sets only the ones its handler reads.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct DoArgs {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub y: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub part: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slot: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outside: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_key: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub row: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub col: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dy: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub k: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub b: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inside: Option<bool>,
    /// The rename buffer Rust owns, handed back whole on commit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// The settings verbs' target: an option key, or the raw key name for `edit_key`/`record_chord`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delta: Option<i64>,
    /// `record_chord` only; settings_model.lua joins the list with `|`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub mods: Vec<&'static str>,
}

impl DoArgs {
    pub fn is_empty(&self) -> bool {
        *self == DoArgs::default()
    }
}

impl Event {
    pub fn ready(cols: u16, rows: u16) -> Self {
        Event::Ready {
            v: VERSION,
            cols,
            rows,
            paints: true,
        }
    }

    pub fn do_(a: &'static str) -> Self {
        Event::Do {
            a,
            id: None,
            args: Box::default(),
        }
    }

    pub fn do_tab(a: &'static str, id: i64) -> Self {
        Event::Do {
            a,
            id: Some(DoId::Tab(id)),
            args: Box::default(),
        }
    }

    /// A verb aimed at something Lua names by string: a strip button, a space.
    pub fn do_named(a: &'static str, id: String) -> Self {
        Event::Do {
            a,
            id: Some(DoId::Name(id)),
            args: Box::default(),
        }
    }

    pub fn with(mut self, set: impl FnOnce(&mut DoArgs)) -> Self {
        if let Event::Do { args, .. } = &mut self {
            set(args.as_mut());
        }
        self
    }

    pub fn paste(data: Option<Vec<u8>>) -> Self {
        match data {
            Some(bytes) => Event::Paste {
                data: Some(b64(&bytes)),
                dropped: None,
            },
            None => Event::Paste {
                data: None,
                dropped: Some("size"),
            },
        }
    }

    pub fn key(name: String, mods: Mods, raw: &[u8]) -> Self {
        Event::Key {
            key: name,
            mods: mods_list(mods),
            raw: (!raw.is_empty()).then(|| b64(raw)),
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("event serializes")
    }
}

pub fn mods_list(mods: Mods) -> Vec<&'static str> {
    let mut list = Vec::new();
    if mods.shift {
        list.push("shift");
    }
    if mods.ctrl {
        list.push("ctrl");
    }
    if mods.alt {
        list.push("alt");
    }
    list
}
