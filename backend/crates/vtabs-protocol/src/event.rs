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
        /// Always true since P8; Lua refuses a backend that announces anything else.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ready_and_resize() {
        assert_eq!(
            Event::ready(30, 40).to_json(),
            r#"{"t":"ready","v":2,"cols":30,"rows":40,"paints":true}"#
        );
        assert_eq!(
            Event::Resize { cols: 31, rows: 40 }.to_json(),
            r#"{"t":"resize","cols":31,"rows":40}"#
        );
    }

    #[test]
    fn do_events_match_the_lua_reader() {
        assert_eq!(
            Event::do_tab("press_card", 7)
                .with(|a| {
                    a.x = Some(5);
                    a.y = Some(6);
                    a.part = Some("title");
                })
                .to_json(),
            r#"{"t":"do","a":"press_card","id":7,"args":{"x":5,"y":6,"part":"title"}}"#
        );
        assert_eq!(
            Event::do_("new_tab").to_json(),
            r#"{"t":"do","a":"new_tab"}"#
        );
        assert_eq!(
            Event::Do {
                a: "strip",
                id: Some(DoId::Name("settings".into())),
                args: Box::default(),
            }
            .to_json(),
            r#"{"t":"do","a":"strip","id":"settings"}"#
        );
        assert_eq!(
            Event::do_("set_scroll")
                .with(|a| {
                    a.top = Some(3);
                    a.user = Some(true);
                })
                .to_json(),
            r#"{"t":"do","a":"set_scroll","args":{"top":3,"user":true}}"#
        );
    }

    #[test]
    fn key_events() {
        assert_eq!(
            Event::key("enter".into(), Mods::default(), b"\r").to_json(),
            r#"{"t":"key","key":"enter","raw":"DQ=="}"#
        );
        let ctrl = Mods {
            ctrl: true,
            ..Mods::default()
        };
        assert_eq!(
            Event::key("c".into(), ctrl, b"\x03").to_json(),
            r#"{"t":"key","key":"c","mods":["ctrl"],"raw":"Aw=="}"#
        );
        assert_eq!(
            Event::key("escape".into(), Mods::default(), b"").to_json(),
            r#"{"t":"key","key":"escape"}"#
        );
    }

    #[test]
    fn focus_and_pong() {
        assert_eq!(
            Event::Focus { focused: true }.to_json(),
            r#"{"t":"focus","in":true}"#
        );
        assert_eq!(
            Event::Focus { focused: false }.to_json(),
            r#"{"t":"focus","in":false}"#
        );
        assert_eq!(
            Event::paste(Some(b"hi".to_vec())).to_json(),
            r#"{"t":"paste","data":"aGk="}"#
        );
        assert_eq!(
            Event::paste(None).to_json(),
            r#"{"t":"paste","dropped":"size"}"#
        );
        assert_eq!(
            Event::Dropped {
                what: "model",
                reason: "bounds"
            }
            .to_json(),
            r#"{"t":"dropped","what":"model","reason":"bounds"}"#
        );
        assert_eq!(Event::Pong { echo: None }.to_json(), r#"{"t":"pong"}"#);
        assert_eq!(
            Event::Note {
                k: "menu_refused",
                why: Some("bounds"),
                id: Some(7),
                a: Some("confirm"),
            }
            .to_json(),
            r#"{"t":"note","k":"menu_refused","why":"bounds","id":7,"a":"confirm"}"#
        );
        // `echo` is the ping's own number; App::emit appends the monotonic `n` on top of it
        assert_eq!(
            Event::Pong { echo: Some(5) }.to_json(),
            r#"{"t":"pong","echo":5}"#
        );
    }
}
