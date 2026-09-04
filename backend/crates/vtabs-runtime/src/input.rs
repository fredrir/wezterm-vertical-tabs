//! Demultiplexes raw stdin bytes into terminal input and framed, session-bound control records.

use vtabs_protocol::limits::{
    CONTROL_PREFIX, FORWARDED_KEY_MAX_BYTES, LINE_MAX, PARSER_BUFFER_MAX, PASTE_MAX_BYTES,
    valid_control_token,
};
use vtabs_protocol::{Button, Command, Mods, Mouse, MouseKind};

const ESC: u8 = 0x1b;

fn mods_from_sgr(cb: u32) -> Mods {
    Mods {
        shift: cb & 4 != 0,
        alt: cb & 8 != 0,
        ctrl: cb & 16 != 0,
    }
}

fn mods_from_csi_param(param: u32) -> Mods {
    let bits = param.saturating_sub(1);
    Mods {
        shift: bits & 1 != 0,
        alt: bits & 2 != 0,
        ctrl: bits & 4 != 0,
    }
}

fn button_from_sgr(cb: u32) -> Button {
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

#[derive(Debug, Clone, PartialEq)]
pub enum Input {
    Mouse(Mouse),
    Focus(bool),
    /// `raw` holds the exact bytes this key was decoded from, for forwarding to another pane.
    Key {
        name: String,
        mods: Mods,
        raw: Vec<u8>,
    },
    /// Bracketed paste; `None` once it grew past the size cap.
    Paste(Option<Vec<u8>>),
    /// A control record separated from keyboard input and bound to one plugin-minted session.
    Control {
        token: String,
        command: Command,
    },
    /// Unit tests for the command state machine bypass transport/auth, which are covered separately.
    #[cfg(test)]
    Command(Command),
    /// A framed control record refused whole; the runtime reports it as `dropped{what,reason}`.
    Dropped {
        token: Option<String>,
        what: &'static str,
        reason: &'static str,
    },
}

#[derive(Default)]
pub struct Parser {
    buf: Vec<u8>,
    stalled_flushes: u32,
    discarding: bool,
    pasting: Option<Vec<u8>>,
}

/// Flushes arrive every ~30ms while input is pending; give up on a stalled prefix after this many.
const STALL_LIMIT: u32 = 10;

const PASTE_END: &[u8] = b"\x1b[201~";

enum Step {
    Token(Input, usize),
    Skip(usize),
    /// Drop these bytes and the rest of the command line they belong to.
    Discard(usize),
    /// Bracketed paste opened: collect raw bytes until the terminator.
    Paste(usize),
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
        self.stalled_flushes = 0;
        if self.pasting.is_none()
            && self.buf.len() > PARSER_BUFFER_MAX
            && !complete_control_batch(&self.buf)
        {
            // A single terminal read must not manufacture an unbounded key flood. The one valid
            // exception is a complete atomic write containing several individually-bounded control
            // records; Lua deliberately sends begin/sections/commit in one `send_text` call.
            self.discarding |= self.buf.starts_with(CONTROL_PREFIX);
            self.buf.clear();
            return Vec::new();
        }
        let out = self.drain(false, false);
        if self.buf.len() > PARSER_BUFFER_MAX {
            // Only an unfinished runaway frame remains after `drain`; completed multiline atomic
            // batches were consumed record by record and are never rejected for their total size.
            self.discarding |= self.buf.starts_with(CONTROL_PREFIX);
            self.buf.clear();
        }
        out
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

    /// Returns false while the paste is still open and more bytes are needed.
    fn collect_paste(&mut self, out: &mut Vec<Input>) -> bool {
        let Some(data) = self.pasting.as_mut() else {
            return true;
        };
        let end = find(&self.buf, PASTE_END);
        let take = end.unwrap_or_else(|| self.buf.len() - partial_tail(&self.buf, PASTE_END));
        if data.len() <= PASTE_MAX_BYTES {
            // Retain one byte beyond the cap as the overflow marker, but never copy an arbitrarily
            // large terminal read into the paste accumulator.
            let remaining = PASTE_MAX_BYTES + 1 - data.len();
            data.extend_from_slice(&self.buf[..take.min(remaining)]);
        }
        self.buf
            .drain(..take + if end.is_some() { PASTE_END.len() } else { 0 });
        if end.is_none() {
            return false;
        }
        let data = self.pasting.take().unwrap_or_default();
        out.push(Input::Paste(if data.len() > PASTE_MAX_BYTES {
            None
        } else {
            Some(data)
        }));
        true
    }

    fn drain(&mut self, timed_out: bool, stalled: bool) -> Vec<Input> {
        let mut out = Vec::new();
        loop {
            if !self.drain_once(timed_out, stalled, &mut out) {
                return out;
            }
        }
    }

    /// Returns true when a mode change (discard, paste) means the rest of the buffer needs another pass.
    fn drain_once(&mut self, timed_out: bool, stalled: bool, out: &mut Vec<Input>) -> bool {
        if self.discarding {
            match self.buf.iter().position(|&b| b == b'\n') {
                Some(end) => {
                    self.buf.drain(..=end);
                    self.discarding = false;
                }
                None => {
                    self.buf.clear();
                    return false;
                }
            }
        }
        if self.pasting.is_some() && !self.collect_paste(out) {
            return false;
        }
        let mut again = false;
        let mut pos = 0;
        while pos < self.buf.len() {
            let wait = Wait { timed_out, stalled };
            match parse_one(&self.buf[pos..], wait) {
                Step::Token(mut input, n) => {
                    if let Input::Key { raw, .. } = &mut input
                        && n <= FORWARDED_KEY_MAX_BYTES
                    {
                        raw.extend_from_slice(&self.buf[pos..pos + n]);
                    }
                    push_coalesced(out, input);
                    pos += n;
                }
                Step::Skip(n) => pos += n,
                Step::Discard(n) => {
                    pos += n;
                    self.discarding = true;
                    again = true;
                    break;
                }
                Step::Paste(n) => {
                    pos += n;
                    self.pasting = Some(Vec::new());
                    again = true;
                    break;
                }
                Step::Incomplete => break,
            }
        }
        self.buf.drain(..pos);
        again
    }
}

/// A large write may exceed the parser's residual-buffer bound only when every byte is part of a
/// complete framed record. Individual JSON payload bounds are enforced by `parse_control_line`.
fn complete_control_batch(bytes: &[u8]) -> bool {
    !bytes.is_empty()
        && bytes.last() == Some(&b'\n')
        && bytes
            .split_inclusive(|&byte| byte == b'\n')
            .all(|line| line.starts_with(CONTROL_PREFIX))
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Length of the trailing bytes that could still grow into `needle` in the next chunk.
fn partial_tail(haystack: &[u8], needle: &[u8]) -> usize {
    let max = needle.len().min(haystack.len());
    (1..max)
        .rev()
        .find(|&n| haystack[haystack.len() - n..] == needle[..n])
        .unwrap_or(0)
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
        byte if byte == CONTROL_PREFIX[0] => parse_control_line(bytes, wait),
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

/// One inbox record: the control prefix, a session token and a JSON payload on a single
/// terminated line. Nothing here ever falls back to a key, so an inbox can never type.
pub fn decode_control_line(line: &[u8]) -> Option<Input> {
    if !line.starts_with(CONTROL_PREFIX) || line.last() != Some(&b'\n') {
        return None;
    }
    let wait = Wait {
        timed_out: true,
        stalled: true,
    };
    match parse_control_line(line, wait) {
        Step::Token(input @ (Input::Control { .. } | Input::Dropped { .. }), n)
            if n == line.len() =>
        {
            Some(input)
        }
        _ => None,
    }
}

fn parse_control_line(bytes: &[u8], wait: Wait) -> Step {
    if bytes.len() < CONTROL_PREFIX.len() && CONTROL_PREFIX.starts_with(bytes) {
        return if wait.stalled {
            Step::Discard(bytes.len())
        } else {
            Step::Incomplete
        };
    }
    if !bytes.starts_with(CONTROL_PREFIX) {
        return parse_plain(bytes);
    }
    let Some(end) = bytes.iter().position(|&b| b == b'\n') else {
        // Abandoned command bytes must never be re-read as input; that would type a frame at the shell.
        return if bytes.len() > PARSER_BUFFER_MAX || wait.stalled {
            Step::Discard(bytes.len())
        } else {
            Step::Incomplete
        };
    };
    let framed = &bytes[CONTROL_PREFIX.len()..end];
    let Some(separator) = framed.iter().position(|&b| b == b' ') else {
        return Step::Skip(end + 1);
    };
    let (token, payload) = (&framed[..separator], &framed[separator + 1..]);
    let Ok(token) = std::str::from_utf8(token) else {
        return Step::Skip(end + 1);
    };
    if !valid_control_token(token) {
        return Step::Skip(end + 1);
    }
    if payload.len() > LINE_MAX {
        return Step::Token(
            Input::Dropped {
                token: Some(token.to_owned()),
                what: "line",
                reason: "size",
            },
            end + 1,
        );
    }
    match serde_json::from_slice::<Command>(payload) {
        Ok(command) => Step::Token(
            Input::Control {
                token: token.to_owned(),
                command,
            },
            end + 1,
        ),
        Err(_) => Step::Token(
            Input::Dropped {
                token: Some(token.to_owned()),
                what: "command",
                reason: "invalid",
            },
            end + 1,
        ),
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
        Some(&ESC | &0x1e) => Step::Token(key("escape", Mods::default()), 1),
        Some(_) => match parse_plain(&bytes[1..]) {
            Step::Token(Input::Key { name, mods, .. }, n) => {
                Step::Token(key(name, Mods { alt: true, ..mods }), n + 1)
            }
            Step::Token(other, n) => Step::Token(other, n + 1),
            Step::Skip(n) => Step::Skip(n + 1),
            Step::Discard(n) => Step::Discard(n + 1),
            Step::Paste(n) => Step::Paste(n + 1),
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
        _ => return Step::Token(unknown_key(), 3),
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
    if body.first() == Some(&b'<') && matches!(final_byte, b'M' | b'm') {
        // an unrecognised mouse report is dropped: it is not text and must never be forwarded
        return match sgr_mouse(&body[1..], final_byte == b'M') {
            Some(t) => Step::Token(t, consumed),
            None => Step::Skip(consumed),
        };
    }
    if final_byte == b'~' {
        match params(body).first() {
            Some(&200) => return Step::Paste(consumed),
            Some(&201) => return Step::Skip(consumed),
            _ => {}
        }
    }
    let token = match (body.first(), final_byte) {
        (None, b'I') => Some(Input::Focus(true)),
        (None, b'O') => Some(Input::Focus(false)),
        (_, b'~') => tilde_key(body),
        (_, b'A' | b'B' | b'C' | b'D' | b'H' | b'F') => letter_key(body, final_byte),
        _ => None,
    };
    Step::Token(token.unwrap_or_else(unknown_key), consumed)
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
    let mods = mods_from_sgr(cb);
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
        let button = button_from_sgr(cb);
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
        .map(|&m| mods_from_csi_param(m))
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
        .map(|&m| mods_from_csi_param(m))
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

/// A sequence the parser cannot name: still forwardable, because `raw` carries the bytes.
fn unknown_key() -> Input {
    key("unknown", Mods::default())
}

fn key(name: impl Into<String>, mods: Mods) -> Input {
    Input::Key {
        name: name.into(),
        mods,
        raw: Vec::new(),
    }
}
