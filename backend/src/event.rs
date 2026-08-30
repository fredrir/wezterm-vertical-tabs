use serde::Serialize;

use crate::parser::{Button, Mods, Mouse, MouseKind};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Event {
    Ready {
        cols: u16,
        rows: u16,
    },
    Resize {
        cols: u16,
        rows: u16,
    },
    Mouse {
        k: &'static str,
        b: &'static str,
        x: u16,
        y: u16,
        #[serde(skip_serializing_if = "Option::is_none")]
        dy: Option<i8>,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        mods: Vec<&'static str>,
    },
    Key {
        key: String,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        mods: Vec<&'static str>,
    },
    Focus {
        #[serde(rename = "in")]
        focused: bool,
    },
    Pong {
        #[serde(skip_serializing_if = "Option::is_none")]
        n: Option<u64>,
    },
}

impl Event {
    pub fn key(name: String, mods: Mods) -> Self {
        Event::Key {
            key: name,
            mods: mods_list(mods),
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("event serializes")
    }
}

impl From<Mouse> for Event {
    fn from(m: Mouse) -> Self {
        let (k, dy) = match m.kind {
            MouseKind::Press => ("down", None),
            MouseKind::Release => ("up", None),
            MouseKind::Drag => ("drag", None),
            MouseKind::Move => ("move", None),
            MouseKind::Wheel => ("wheel", Some(m.dy)),
        };
        Event::Mouse {
            k,
            b: button_name(m.button),
            x: m.x,
            y: m.y,
            dy,
            mods: mods_list(m.mods),
        }
    }
}

fn button_name(button: Button) -> &'static str {
    match button {
        Button::Left => "left",
        Button::Middle => "middle",
        Button::Right => "right",
        Button::None => "none",
    }
}

fn mods_list(mods: Mods) -> Vec<&'static str> {
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

    fn mouse(kind: MouseKind, button: Button, dy: i8, mods: Mods) -> Mouse {
        Mouse {
            kind,
            button,
            x: 3,
            y: 2,
            dy,
            mods,
        }
    }

    #[test]
    fn ready_and_resize() {
        assert_eq!(
            Event::Ready { cols: 30, rows: 40 }.to_json(),
            r#"{"t":"ready","cols":30,"rows":40}"#
        );
        assert_eq!(
            Event::Resize { cols: 31, rows: 40 }.to_json(),
            r#"{"t":"resize","cols":31,"rows":40}"#
        );
    }

    #[test]
    fn mouse_down_without_mods() {
        let ev = Event::from(mouse(MouseKind::Press, Button::Left, 0, Mods::default()));
        assert_eq!(
            ev.to_json(),
            r#"{"t":"mouse","k":"down","b":"left","x":3,"y":2}"#
        );
    }

    #[test]
    fn mouse_up_with_mods_in_spec_order() {
        let ev = Event::from(mouse(
            MouseKind::Release,
            Button::Right,
            0,
            Mods {
                shift: true,
                alt: true,
                ctrl: true,
            },
        ));
        assert_eq!(
            ev.to_json(),
            r#"{"t":"mouse","k":"up","b":"right","x":3,"y":2,"mods":["shift","ctrl","alt"]}"#
        );
    }

    #[test]
    fn mouse_drag_and_move() {
        let drag = Event::from(mouse(MouseKind::Drag, Button::Middle, 0, Mods::default()));
        assert_eq!(
            drag.to_json(),
            r#"{"t":"mouse","k":"drag","b":"middle","x":3,"y":2}"#
        );
        let mv = Event::from(mouse(MouseKind::Move, Button::None, 0, Mods::default()));
        assert_eq!(
            mv.to_json(),
            r#"{"t":"mouse","k":"move","b":"none","x":3,"y":2}"#
        );
    }

    #[test]
    fn wheel_carries_dy_and_no_button() {
        let ev = Event::from(mouse(
            MouseKind::Wheel,
            Button::None,
            -1,
            Mods {
                ctrl: true,
                ..Mods::default()
            },
        ));
        assert_eq!(
            ev.to_json(),
            r#"{"t":"mouse","k":"wheel","b":"none","x":3,"y":2,"dy":-1,"mods":["ctrl"]}"#
        );
    }

    #[test]
    fn key_events() {
        assert_eq!(
            Event::key("enter".into(), Mods::default()).to_json(),
            r#"{"t":"key","key":"enter"}"#
        );
        let ctrl = Mods {
            ctrl: true,
            ..Mods::default()
        };
        assert_eq!(
            Event::key("c".into(), ctrl).to_json(),
            r#"{"t":"key","key":"c","mods":["ctrl"]}"#
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
        assert_eq!(Event::Pong { n: None }.to_json(), r#"{"t":"pong"}"#);
        assert_eq!(
            Event::Pong { n: Some(5) }.to_json(),
            r#"{"t":"pong","n":5}"#
        );
    }
}
