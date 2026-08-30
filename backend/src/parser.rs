//! Demultiplexes raw stdin bytes into mouse/key/focus tokens and JSON command lines.

use crate::command::Command;

const ESC: u8 = 0x1b;
const MAX_LINE: usize = 1 << 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Mods {
    pub shift: bool,
    pub alt: bool,
    pub ctrl: bool,
}

impl Mods {
    fn from_sgr(cb: u32) -> Self {
        Self {
            shift: cb & 4 != 0,
            alt: cb & 8 != 0,
            ctrl: cb & 16 != 0,
        }
    }

    fn from_csi_param(param: u32) -> Self {
        let bits = param.saturating_sub(1);
        Self {
            shift: bits & 1 != 0,
            alt: bits & 2 != 0,
            ctrl: bits & 4 != 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseKind {
    Press,
    Release,
    Drag,
    Move,
    Wheel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Left,
    Middle,
    Right,
    None,
}

impl Button {
    fn from_sgr(cb: u32) -> Self {
        if cb & 128 != 0 {
            return Button::None;
        }
        match cb & 3 {
            0 => Button::Left,
            1 => Button::Middle,
            2 => Button::Right,
            _ => Button::None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mouse {
    pub kind: MouseKind,
    pub button: Button,
    pub x: u16,
    pub y: u16,
    /// Only meaningful for `Wheel`: -1 up, 1 down.
    pub dy: i8,
    pub mods: Mods,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    Mouse(Mouse),
    Focus(bool),
    Key { name: String, mods: Mods },
    Command(Command),
}

#[derive(Default)]
pub struct Parser {
    buf: Vec<u8>,
    stalled_flushes: u32,
}

/// Flushes arrive every ~30ms while input is pending; give up on a stalled prefix after this many.
const STALL_LIMIT: u32 = 10;

enum Step {
    Token(Input, usize),
    Skip(usize),
    Incomplete,
}

/// `timed_out`: no bytes for ~30ms (bare ESC → escape). `stalled`: a prefix has waited ~300ms.
#[derive(Clone, Copy)]
struct Wait {
    timed_out: bool,
    stalled: bool,
}

impl Parser {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn feed(&mut self, bytes: &[u8]) -> Vec<Input> {
        self.buf.extend_from_slice(bytes);
        if self.buf.len() > MAX_LINE {
            self.buf.clear();
        }
        self.stalled_flushes = 0;
        self.drain(false, false)
    }

    /// Called after a quiet period: a bare ESC becomes the escape key; a stalled prefix is abandoned.
    pub fn flush(&mut self) -> Vec<Input> {
        self.stalled_flushes = self.stalled_flushes.saturating_add(1);
        let stalled = self.stalled_flushes >= STALL_LIMIT;
        if stalled {
            self.stalled_flushes = 0;
        }
        self.drain(true, stalled)
    }

    pub fn has_pending(&self) -> bool {
        !self.buf.is_empty()
    }

    fn drain(&mut self, timed_out: bool, stalled: bool) -> Vec<Input> {
        let mut out = Vec::new();
        let mut pos = 0;
        while pos < self.buf.len() {
            let wait = Wait { timed_out, stalled };
            match parse_one(&self.buf[pos..], wait) {
                Step::Token(input, n) => {
                    push_coalesced(&mut out, input);
                    pos += n;
                }
                Step::Skip(n) => pos += n,
                Step::Incomplete => break,
            }
        }
        self.buf.drain(..pos);
        out
    }
}

fn push_coalesced(out: &mut Vec<Input>, input: Input) {
    if let (Some(Input::Mouse(prev)), Input::Mouse(next)) = (out.last_mut(), &input)
        && matches!(next.kind, MouseKind::Move | MouseKind::Drag)
        && prev.kind == next.kind
        && prev.button == next.button
        && prev.mods == next.mods
    {
        *prev = *next;
        return;
    }
    out.push(input);
}

fn parse_one(bytes: &[u8], wait: Wait) -> Step {
    match bytes[0] {
        b'{' => parse_command_line(bytes, wait),
        ESC => parse_escape(bytes, wait),
        _ => parse_plain(bytes),
    }
}

fn parse_plain(bytes: &[u8]) -> Step {
    let b = bytes[0];
    if b.is_ascii() {
        return Step::Token(ascii_key(b), 1);
    }
    let len = utf8_len(b);
    if len == 0 {
        return Step::Skip(1);
    }
    if bytes.len() < len {
        return Step::Incomplete;
    }
    match std::str::from_utf8(&bytes[..len]) {
        Ok(ch) => Step::Token(key(ch, Mods::default()), len),
        Err(_) => Step::Skip(1),
    }
}

fn utf8_len(lead: u8) -> usize {
    match lead {
        0xc2..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf4 => 4,
        _ => 0,
    }
}

fn parse_command_line(bytes: &[u8], wait: Wait) -> Step {
    let Some(end) = bytes.iter().position(|&b| b == b'\n') else {
        return if bytes.len() > MAX_LINE || wait.stalled {
            Step::Skip(1)
        } else {
            Step::Incomplete
        };
    };
    let line = &bytes[..end];
    match serde_json::from_slice::<Command>(line) {
        Ok(cmd) => Step::Token(Input::Command(cmd), end + 1),
        Err(_) => Step::Skip(end + 1),
    }
}

fn parse_escape(bytes: &[u8], wait: Wait) -> Step {
    match bytes.get(1) {
        None => {
            if wait.timed_out {
                Step::Token(key("escape", Mods::default()), 1)
            } else {
                Step::Incomplete
            }
        }
        Some(b'[') => parse_csi(bytes, wait),
        Some(b'O') => parse_ss3(bytes, wait),
        Some(&ESC | &b'{') => Step::Token(key("escape", Mods::default()), 1),
        Some(_) => match parse_plain(&bytes[1..]) {
            Step::Token(Input::Key { name, mods }, n) => {
                Step::Token(key(name, Mods { alt: true, ..mods }), n + 1)
            }
            Step::Token(other, n) => Step::Token(other, n + 1),
            Step::Skip(n) => Step::Skip(n + 1),
            Step::Incomplete => Step::Incomplete,
        },
    }
}

fn parse_ss3(bytes: &[u8], wait: Wait) -> Step {
    let Some(&final_byte) = bytes.get(2) else {
        return escape_or_wait(wait);
    };
    let name = match final_byte {
        b'A' => "up",
        b'B' => "down",
        b'C' => "right",
        b'D' => "left",
        b'H' => "home",
        b'F' => "end",
        _ => return Step::Skip(3),
    };
    Step::Token(key(name, Mods::default()), 3)
}

/// Scans CSI parameter/intermediate bytes for the final byte; `{`/`}` and controls abort the sequence.
fn csi_end(bytes: &[u8]) -> Result<Option<usize>, ()> {
    for (i, &b) in bytes.iter().enumerate().skip(2) {
        match b {
            0x20..=0x3f => continue,
            b'{' | b'}' => return Err(()),
            0x40..=0x7e => return Ok(Some(i)),
            _ => return Err(()),
        }
    }
    Ok(None)
}

fn parse_csi(bytes: &[u8], wait: Wait) -> Step {
    let end = match csi_end(bytes) {
        Ok(Some(end)) => end,
        Ok(None) => return escape_or_wait(wait),
        Err(()) => return Step::Token(key("escape", Mods::default()), 1),
    };
    let final_byte = bytes[end];
    let body = &bytes[2..end];
    let consumed = end + 1;
    let token = match (body.first(), final_byte) {
        (Some(b'<'), b'M' | b'm') => sgr_mouse(&body[1..], final_byte == b'M'),
        (None, b'I') => Some(Input::Focus(true)),
        (None, b'O') => Some(Input::Focus(false)),
        (_, b'~') => tilde_key(body),
        (_, b'A' | b'B' | b'C' | b'D' | b'H' | b'F') => letter_key(body, final_byte),
        _ => None,
    };
    match token {
        Some(t) => Step::Token(t, consumed),
        None => Step::Skip(consumed),
    }
}

/// An `ESC [`/`ESC O` introducer promises more bytes, so only a long stall turns it into escape.
fn escape_or_wait(wait: Wait) -> Step {
    if wait.stalled {
        Step::Token(key("escape", Mods::default()), 1)
    } else {
        Step::Incomplete
    }
}

fn params(body: &[u8]) -> Vec<u32> {
    body.split(|&b| b == b';')
        .map(|p| {
            std::str::from_utf8(p)
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0)
        })
        .collect()
}

fn sgr_mouse(body: &[u8], press: bool) -> Option<Input> {
    let p = params(body);
    let (&cb, &x, &y) = (p.first()?, p.get(1)?, p.get(2)?);
    let (x, y) = (x as u16, y as u16);
    let mods = Mods::from_sgr(cb);
    let mouse = if cb & 64 != 0 {
        if cb & 3 >= 2 {
            return None;
        }
        let dy = if cb & 1 == 0 { -1 } else { 1 };
        Mouse {
            kind: MouseKind::Wheel,
            button: Button::None,
            x,
            y,
            dy,
            mods,
        }
    } else {
        let button = Button::from_sgr(cb);
        let kind = match (cb & 32 != 0, press, button) {
            (true, _, Button::None) => MouseKind::Move,
            (true, _, _) => MouseKind::Drag,
            (false, true, _) => MouseKind::Press,
            (false, false, _) => MouseKind::Release,
        };
        Mouse {
            kind,
            button,
            x,
            y,
            dy: 0,
            mods,
        }
    };
    Some(Input::Mouse(mouse))
}

fn tilde_key(body: &[u8]) -> Option<Input> {
    let p = params(body);
    let name = match p.first()? {
        1 | 7 => "home",
        3 => "delete",
        4 | 8 => "end",
        5 => "pageup",
        6 => "pagedown",
        _ => return None,
    };
    let mods = p
        .get(1)
        .map(|&m| Mods::from_csi_param(m))
        .unwrap_or_default();
    Some(key(name, mods))
}

fn letter_key(body: &[u8], final_byte: u8) -> Option<Input> {
    let name = match final_byte {
        b'A' => "up",
        b'B' => "down",
        b'C' => "right",
        b'D' => "left",
        b'H' => "home",
        b'F' => "end",
        _ => return None,
    };
    let mods = params(body)
        .get(1)
        .map(|&m| Mods::from_csi_param(m))
        .unwrap_or_default();
    Some(key(name, mods))
}

fn ascii_key(b: u8) -> Input {
    let ctrl = Mods {
        ctrl: true,
        ..Mods::default()
    };
    match b {
        b'\r' | b'\n' => key("enter", Mods::default()),
        b'\t' => key("tab", Mods::default()),
        0x7f | 0x08 => key("backspace", Mods::default()),
        b' ' => key("space", Mods::default()),
        0x00 => key("space", ctrl),
        0x01..=0x1a => key(((b'a' + b - 1) as char).to_string(), ctrl),
        0x1c..=0x1f => key(((b'\\' + b - 0x1c) as char).to_string(), ctrl),
        b => key((b as char).to_string(), Mods::default()),
    }
}

fn key(name: impl Into<String>, mods: Mods) -> Input {
    Input::Key {
        name: name.into(),
        mods,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(bytes: &[u8]) -> Vec<Input> {
        Parser::new().feed(bytes)
    }

    fn mouse(kind: MouseKind, button: Button, x: u16, y: u16, dy: i8, mods: Mods) -> Input {
        Input::Mouse(Mouse {
            kind,
            button,
            x,
            y,
            dy,
            mods,
        })
    }

    fn plain(name: &str) -> Input {
        key(name, Mods::default())
    }

    #[test]
    fn sgr_press_release() {
        assert_eq!(
            feed(b"\x1b[<0;3;2M\x1b[<0;3;2m"),
            vec![
                mouse(MouseKind::Press, Button::Left, 3, 2, 0, Mods::default()),
                mouse(MouseKind::Release, Button::Left, 3, 2, 0, Mods::default()),
            ]
        );
    }

    #[test]
    fn sgr_buttons_and_modifiers() {
        let shift_ctrl = Mods {
            shift: true,
            alt: false,
            ctrl: true,
        };
        assert_eq!(
            feed(b"\x1b[<22;1;1M"),
            vec![mouse(MouseKind::Press, Button::Right, 1, 1, 0, shift_ctrl)]
        );
        let alt = Mods {
            alt: true,
            ..Mods::default()
        };
        assert_eq!(
            feed(b"\x1b[<9;7;8M"),
            vec![mouse(MouseKind::Press, Button::Middle, 7, 8, 0, alt)]
        );
    }

    #[test]
    fn sgr_drag_and_move() {
        assert_eq!(
            feed(b"\x1b[<32;5;6M"),
            vec![mouse(
                MouseKind::Drag,
                Button::Left,
                5,
                6,
                0,
                Mods::default()
            )]
        );
        assert_eq!(
            feed(b"\x1b[<35;5;6M"),
            vec![mouse(
                MouseKind::Move,
                Button::None,
                5,
                6,
                0,
                Mods::default()
            )]
        );
    }

    #[test]
    fn sgr_wheel() {
        assert_eq!(
            feed(b"\x1b[<64;2;3M"),
            vec![mouse(
                MouseKind::Wheel,
                Button::None,
                2,
                3,
                -1,
                Mods::default()
            )]
        );
        let ctrl = Mods {
            ctrl: true,
            ..Mods::default()
        };
        assert_eq!(
            feed(b"\x1b[<81;2;3M"),
            vec![mouse(MouseKind::Wheel, Button::None, 2, 3, 1, ctrl)]
        );
    }

    #[test]
    fn move_coalescing_keeps_last() {
        let out = feed(b"\x1b[<35;1;1M\x1b[<35;2;2M\x1b[<35;3;3M\x1b[<32;4;4M\x1b[<32;5;5M");
        assert_eq!(
            out,
            vec![
                mouse(MouseKind::Move, Button::None, 3, 3, 0, Mods::default()),
                mouse(MouseKind::Drag, Button::Left, 5, 5, 0, Mods::default()),
            ]
        );
    }

    #[test]
    fn focus_in_out() {
        assert_eq!(
            feed(b"\x1b[I\x1b[O"),
            vec![Input::Focus(true), Input::Focus(false)]
        );
    }

    #[test]
    fn navigation_keys() {
        let out = feed(b"\x1b[A\x1b[B\x1b[C\x1b[D\x1b[H\x1b[F\x1b[5~\x1b[6~\x1b[3~\x1bOH\x1b[4~");
        let names: Vec<_> = out
            .iter()
            .map(|i| match i {
                Input::Key { name, .. } => name.as_str(),
                _ => panic!("not a key"),
            })
            .collect();
        assert_eq!(
            names,
            [
                "up", "down", "right", "left", "home", "end", "pageup", "pagedown", "delete",
                "home", "end"
            ]
        );
    }

    #[test]
    fn modified_navigation_keys() {
        let shift = Mods {
            shift: true,
            ..Mods::default()
        };
        let ctrl_alt = Mods {
            alt: true,
            ctrl: true,
            shift: false,
        };
        assert_eq!(feed(b"\x1b[1;2A"), vec![key("up", shift)]);
        assert_eq!(feed(b"\x1b[3;7~"), vec![key("delete", ctrl_alt)]);
    }

    #[test]
    fn plain_keys() {
        assert_eq!(
            feed(b"\r\n\t\x7f\x08 x"),
            vec![
                plain("enter"),
                plain("enter"),
                plain("tab"),
                plain("backspace"),
                plain("backspace"),
                plain("space"),
                plain("x"),
            ]
        );
    }

    #[test]
    fn ctrl_letters() {
        let ctrl = Mods {
            ctrl: true,
            ..Mods::default()
        };
        assert_eq!(
            feed(b"\x01\x03\x1a"),
            vec![key("a", ctrl), key("c", ctrl), key("z", ctrl)]
        );
    }

    #[test]
    fn alt_letter() {
        assert_eq!(
            feed(b"\x1bx"),
            vec![key(
                "x",
                Mods {
                    alt: true,
                    ..Mods::default()
                }
            )]
        );
    }

    #[test]
    fn escape_before_command_line() {
        assert_eq!(
            feed(b"\x1b{\"t\":\"ping\"}\n"),
            vec![plain("escape"), Input::Command(Command::Ping { n: None })]
        );
    }

    #[test]
    fn utf8_chars_and_split_sequence() {
        assert_eq!(feed("æ→".as_bytes()), vec![plain("æ"), plain("→")]);
        let mut p = Parser::new();
        assert!(p.feed(&"ø".as_bytes()[..1]).is_empty());
        assert_eq!(p.feed(&"ø".as_bytes()[1..]), vec![plain("ø")]);
        assert_eq!(feed(b"\xffa"), vec![plain("a")]);
    }

    #[test]
    fn unknown_commands_ignored() {
        assert_eq!(
            feed(b"{\"t\":\"bogus\"}\n{\"t\":\"clear\"}\n"),
            vec![Input::Command(Command::Clear)]
        );
        assert_eq!(
            feed(b"{\"t\":\"frame\",\"data\":\"\\u001b[H\"}\n"),
            vec![Input::Command(Command::Frame {
                data: "\x1b[H".into()
            })]
        );
    }

    #[test]
    fn bare_escape_waits_then_flushes() {
        let mut p = Parser::new();
        assert!(p.feed(b"\x1b").is_empty());
        assert!(p.has_pending());
        assert_eq!(p.flush(), vec![plain("escape")]);
        assert!(!p.has_pending());
    }

    #[test]
    fn escape_then_csi_across_chunks() {
        let mut p = Parser::new();
        assert!(p.feed(b"\x1b[<0;").is_empty());
        assert_eq!(
            p.feed(b"3;2M"),
            vec![mouse(
                MouseKind::Press,
                Button::Left,
                3,
                2,
                0,
                Mods::default()
            )]
        );
    }

    #[test]
    fn command_line_split_across_chunks() {
        let mut p = Parser::new();
        assert!(p.feed(b"{\"t\":\"pi").is_empty());
        assert_eq!(
            p.feed(b"ng\"}\n"),
            vec![Input::Command(Command::Ping { n: None })]
        );
    }

    #[test]
    fn command_line_followed_by_mouse_in_same_chunk() {
        assert_eq!(
            feed(b"{\"t\":\"ping\"}\n\x1b[<0;3;2M"),
            vec![
                Input::Command(Command::Ping { n: None }),
                mouse(MouseKind::Press, Button::Left, 3, 2, 0, Mods::default())
            ]
        );
    }

    #[test]
    fn malformed_json_ignored() {
        assert_eq!(
            feed(b"{not json}\n{\"t\":\"nope\"}\n{\"t\":\"quit\"}\n"),
            vec![Input::Command(Command::Quit)]
        );
    }

    #[test]
    fn unknown_csi_skipped() {
        assert_eq!(feed(b"\x1b[?1;2c\x1b[A"), vec![plain("up")]);
    }

    fn flush_until_stalled(p: &mut Parser) -> Vec<Input> {
        let mut out = Vec::new();
        for _ in 0..STALL_LIMIT {
            out.extend(p.flush());
        }
        out
    }

    #[test]
    fn brace_is_not_a_csi_terminator() {
        let out = feed(b"\x1b[{\"t\":\"clear\"}\n");
        assert_eq!(out[0], key("escape", Mods::default()));
        assert_eq!(out[1], key("[", Mods::default()));
        assert_eq!(out[2], Input::Command(Command::Clear));
        assert_eq!(out.len(), 3);
    }

    #[test]
    fn split_sgr_survives_a_short_timeout() {
        let mut p = Parser::new();
        assert!(p.feed(b"\x1b[<0;").is_empty());
        assert!(p.flush().is_empty());
        assert!(p.flush().is_empty());
        let out = p.feed(b"3;2M");
        assert_eq!(
            out,
            vec![mouse(
                MouseKind::Press,
                Button::Left,
                3,
                2,
                0,
                Mods::default()
            )]
        );
    }

    #[test]
    fn stalled_csi_prefix_becomes_escape() {
        let mut p = Parser::new();
        assert!(p.feed(b"\x1b[").is_empty());
        let out = flush_until_stalled(&mut p);
        assert_eq!(
            out,
            vec![key("escape", Mods::default()), key("[", Mods::default())]
        );
        assert!(!p.has_pending());
    }

    #[test]
    fn stray_brace_is_dropped_after_stall() {
        let mut p = Parser::new();
        assert!(p.feed(b"{\x1b[<0;3;2M").is_empty());
        let out = flush_until_stalled(&mut p);
        assert_eq!(
            out,
            vec![mouse(
                MouseKind::Press,
                Button::Left,
                3,
                2,
                0,
                Mods::default()
            )]
        );
    }

    #[test]
    fn extra_buttons_are_none_not_left_or_middle() {
        let out = feed(b"\x1b[<128;5;3M\x1b[<129;5;3M");
        assert_eq!(
            out,
            vec![
                mouse(MouseKind::Press, Button::None, 5, 3, 0, Mods::default()),
                mouse(MouseKind::Press, Button::None, 5, 3, 0, Mods::default()),
            ]
        );
    }

    #[test]
    fn horizontal_wheel_is_ignored() {
        assert!(feed(b"\x1b[<66;5;3M\x1b[<67;5;3M").is_empty());
    }

    #[test]
    fn focus_requires_empty_params() {
        assert!(feed(b"\x1b[5I").is_empty());
        assert_eq!(feed(b"\x1b[I"), vec![Input::Focus(true)]);
    }

    #[test]
    fn oversized_line_is_dropped_without_key_flood() {
        let mut p = Parser::new();
        let mut big = vec![b'{'];
        big.resize(MAX_LINE + 2, b'x');
        assert!(p.feed(&big).is_empty());
        assert!(!p.has_pending());
    }
}
