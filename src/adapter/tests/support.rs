use super::*;
use window::KeyCode;

pub(super) fn raw(key: KeyCode, mods: window::Modifiers, down: bool) -> window::RawKeyEvent {
    window::RawKeyEvent {
        key,
        modifiers: mods,
        leds: Default::default(),
        phys_code: None,
        raw_code: 0,
        #[cfg(windows)]
        scan_code: 0,
        repeat_count: 1,
        key_is_down: down,
        handled: window::Handled::new(),
    }
}

pub(super) fn geometry() -> Geometry {
    Geometry {
        sidebar: Bounds {
            x: 0.,
            y: 0.,
            width: 256.,
            height: 480.,
        },
        content: Bounds {
            x: 256.,
            y: 0.,
            width: 800.,
            height: 480.,
        },
        cell_width: 8.,
        cell_height: 20.,
        dpi: 96.,
        ..Geometry::default()
    }
}

pub(super) fn logical_key(key: KeyCode, mods: window::Modifiers) -> window::KeyEvent {
    window::KeyEvent {
        key,
        modifiers: mods,
        leds: Default::default(),
        repeat_count: 1,
        key_is_down: true,
        raw: None,
        #[cfg(windows)]
        win32_uni_char: None,
    }
}

pub(super) fn paste_token(adapter: &mut Adapter) -> u64 {
    adapter
        .commands()
        .into_iter()
        .find_map(|command| match command {
            Command::Paste(token) => Some(token),
            _ => None,
        })
        .expect("clipboard read request")
}

pub(super) fn copied_text(adapter: &mut Adapter) -> String {
    adapter
        .commands()
        .into_iter()
        .find_map(|command| match command {
            Command::Clipboard(text) => Some(text),
            _ => None,
        })
        .expect("clipboard write request")
}
