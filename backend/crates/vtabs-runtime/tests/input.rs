use vtabs_protocol::limits::{
    FORWARDED_KEY_MAX_BYTES, LINE_MAX, PARSER_BUFFER_MAX, PASTE_MAX_BYTES,
};
use vtabs_protocol::{Button, Command, Mods, Mouse, MouseKind};
use vtabs_runtime::input::{Input, Parser, decode_control_line};

const STALL_LIMIT: u32 = 10;
const TOKEN: &str = "abc123";

fn control(command: Command) -> Input {
    Input::Control {
        token: TOKEN.into(),
        command,
    }
}

fn frame(json: &str) -> Vec<u8> {
    format!("\x1eVTABS {TOKEN} {json}\n").into_bytes()
}

fn key(name: impl Into<String>, mods: Mods) -> Input {
    Input::Key {
        name: name.into(),
        mods,
        raw: Vec::new(),
    }
}

#[test]
fn a_command_line_past_line_max_is_dropped_whole_and_reported() {
    let mut p = Parser::new();
    let mut payload = br#"{"t":"model","rev":1,"tabs":["#.to_vec();
    while payload.len() < LINE_MAX + 16 {
        payload.extend_from_slice(br#"{"id":1,"index":1},"#);
    }
    payload.extend_from_slice(b"]}");
    let mut line = format!("\x1eVTABS {TOKEN} ").into_bytes();
    line.extend(payload);
    line.push(b'\n');
    let out = p.feed(&line);
    assert_eq!(
        out,
        vec![Input::Dropped {
            token: Some(TOKEN.into()),
            what: "line",
            reason: "size"
        }],
        "one refusal, no keys, nothing applied"
    );
    let next = p.feed(b"x");
    assert!(
        matches!(next.as_slice(), [Input::Key { name, .. }] if name == "x"),
        "and the stream continues: {next:?}"
    );
}

/// Existing expectations are written without `raw`; dedicated tests below cover it.
fn bare(inputs: Vec<Input>) -> Vec<Input> {
    inputs
        .into_iter()
        .map(|i| match i {
            Input::Key { name, mods, .. } => key(name, mods),
            other => other,
        })
        .collect()
}

fn feed(bytes: &[u8]) -> Vec<Input> {
    bare(Parser::new().feed(bytes))
}

fn feed_into(p: &mut Parser, bytes: &[u8]) -> Vec<Input> {
    bare(p.feed(bytes))
}

fn raw_of(inputs: &[Input]) -> Vec<&[u8]> {
    inputs
        .iter()
        .filter_map(|i| match i {
            Input::Key { raw, .. } => Some(raw.as_slice()),
            _ => None,
        })
        .collect()
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
            "up", "down", "right", "left", "home", "end", "pageup", "pagedown", "delete", "home",
            "end"
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
    let mut input = vec![0x1b];
    input.extend(frame(r#"{"t":"ping"}"#));
    assert_eq!(
        feed(&input),
        vec![plain("escape"), control(Command::Ping { n: None })]
    );
}

#[test]
fn utf8_chars_and_split_sequence() {
    assert_eq!(feed("æ→".as_bytes()), vec![plain("æ"), plain("→")]);
    let mut p = Parser::new();
    assert!(p.feed(&"ø".as_bytes()[..1]).is_empty());
    assert_eq!(bare(p.feed(&"ø".as_bytes()[1..])), vec![plain("ø")]);
    assert_eq!(feed(b"\xffa"), vec![plain("a")]);
}

#[test]
fn unknown_commands_ignored() {
    let mut input = frame(r#"{"t":"bogus"}"#);
    input.extend(frame(r#"{"t":"clear"}"#));
    assert_eq!(feed(&input), vec![control(Command::Clear)]);
    assert!(feed(&frame(r#"{"t":"frame","data":"\u001b[H"}"#)).is_empty());
    assert!(feed(&frame(r#"{"t":"anim","id":1,"data":"x"}"#)).is_empty());
}

#[test]
fn literal_json_and_braces_are_keyboard_input_not_control() {
    let out = feed(b"{\"t\":\"quit\"}\n{\"t\":\"clear\"}");
    assert_eq!(out.first(), Some(&plain("{")));
    assert!(
        out.iter()
            .all(|item| !matches!(item, Input::Control { .. }))
    );
    assert!(out.iter().any(|item| item == &plain("enter")));
}

#[test]
fn a_control_record_requires_a_bounded_ascii_session_token() {
    assert!(feed(b"\x1eVTABS  {\"t\":\"quit\"}\n").is_empty());
    assert!(feed(b"\x1eVTABS bad\ttoken {\"t\":\"quit\"}\n").is_empty());
    let oversized = "x".repeat(65);
    assert!(feed(format!("\x1eVTABS {oversized} {{\"t\":\"quit\"}}\n").as_bytes()).is_empty());
}

#[test]
fn a_multiline_batch_is_bounded_per_record_not_by_aggregate_write_size() {
    let padding = "x".repeat(800 * 1024);
    let one = frame(&format!(r#"{{"t":"ping","pad":"{padding}"}}"#));
    let mut batch = Vec::new();
    for _ in 0..3 {
        batch.extend_from_slice(&one);
    }
    assert!(batch.len() > PARSER_BUFFER_MAX);
    assert_eq!(
        feed(&batch),
        vec![
            control(Command::Ping { n: None }),
            control(Command::Ping { n: None }),
            control(Command::Ping { n: None }),
        ]
    );
}

#[test]
fn bare_escape_waits_then_flushes() {
    let mut p = Parser::new();
    assert!(p.feed(b"\x1b").is_empty());
    assert!(p.has_pending());
    assert_eq!(bare(p.flush()), vec![plain("escape")]);
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
    assert!(
        p.feed(format!("\x1eVTABS {TOKEN} {{\"t\":\"pi").as_bytes())
            .is_empty()
    );
    assert_eq!(p.feed(b"ng\"}\n"), vec![control(Command::Ping { n: None })]);
}

#[test]
fn command_line_followed_by_mouse_in_same_chunk() {
    let mut input = frame(r#"{"t":"ping"}"#);
    input.extend_from_slice(b"\x1b[<0;3;2M");
    assert_eq!(
        feed(&input),
        vec![
            control(Command::Ping { n: None }),
            mouse(MouseKind::Press, Button::Left, 3, 2, 0, Mods::default())
        ]
    );
}

#[test]
fn malformed_json_ignored() {
    let mut input = frame("{not json}");
    input.extend(frame(r#"{"t":"nope"}"#));
    input.extend(frame(r#"{"t":"quit"}"#));
    assert_eq!(feed(&input), vec![control(Command::Quit)]);
}

#[test]
fn unnamed_csi_becomes_the_unknown_key() {
    assert_eq!(
        feed(b"\x1b[?1;2c\x1b[A"),
        vec![plain("unknown"), plain("up")]
    );
    assert_eq!(feed(b"\x1b[15~"), vec![plain("unknown")]);
    assert_eq!(feed(b"\x1bOZ"), vec![plain("unknown")]);
    let out = Parser::new().feed(b"\x1b[15~");
    assert_eq!(raw_of(&out), vec![b"\x1b[15~".as_slice()]);
}

fn flush_until_stalled(p: &mut Parser) -> Vec<Input> {
    let mut out = Vec::new();
    for _ in 0..STALL_LIMIT {
        out.extend(p.flush());
    }
    bare(out)
}

#[test]
fn brace_is_not_a_csi_terminator() {
    let out = feed(b"\x1b[{\"t\":\"clear\"}\n");
    assert_eq!(out[0], key("escape", Mods::default()));
    assert_eq!(out[1], key("[", Mods::default()));
    assert_eq!(out[2], plain("{"));
    assert!(
        out.iter()
            .all(|item| !matches!(item, Input::Control { .. }))
    );
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
fn stalled_command_line_is_dropped_whole() {
    let mut p = Parser::new();
    assert!(
        p.feed(
            format!(
                "\x1eVTABS {TOKEN} {{\"t\":\"model\",\"rev\":1,\"tabs\":[{{\"title\":\"\x1b[1;1Hx"
            )
            .as_bytes()
        )
        .is_empty()
    );
    assert!(flush_until_stalled(&mut p).is_empty());
    assert!(!p.has_pending());
}

#[test]
fn a_command_line_split_by_a_long_gap_yields_no_keys() {
    let mut p = Parser::new();
    assert!(
        p.feed(
            format!(
                "\x1eVTABS {TOKEN} {{\"t\":\"model\",\"rev\":1,\"tabs\":[{{\"title\":\"\x1b[1;1Hab"
            )
            .as_bytes()
        )
        .is_empty()
    );
    assert!(flush_until_stalled(&mut p).is_empty());
    assert!(p.feed(b"c\x1b[2;1Hd\"}]}\n").is_empty());
    assert_eq!(feed_into(&mut p, b"x"), vec![plain("x")]);
}

#[test]
fn key_events_carry_their_exact_bytes() {
    let out = Parser::new().feed(b"x\x1b[A\x1b\x1bOH\xc3\xa6");
    assert_eq!(
        raw_of(&out),
        vec![
            b"x".as_slice(),
            b"\x1b[A".as_slice(),
            b"\x1b".as_slice(),
            b"\x1bOH".as_slice(),
            "æ".as_bytes(),
        ]
    );
    let mut p = Parser::new();
    assert!(p.feed(b"\x1b").is_empty());
    assert_eq!(raw_of(&p.flush()), vec![b"\x1b".as_slice()]);
}

#[test]
fn a_named_key_past_the_forwarding_cap_carries_no_raw_bytes() {
    let mut sequence = b"\x1b[".to_vec();
    while sequence.len() <= FORWARDED_KEY_MAX_BYTES {
        sequence.extend_from_slice(b"1;");
    }
    sequence.push(b'A');
    let out = Parser::new().feed(&sequence);
    assert!(
        matches!(out.as_slice(), [Input::Key { name, raw, .. }] if name == "up" && raw.is_empty())
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
    assert_eq!(feed(b"\x1b[5I"), vec![plain("unknown")]);
    assert_eq!(feed(b"\x1b[I"), vec![Input::Focus(true)]);
}

#[test]
fn bracketed_paste_is_one_event_never_keys() {
    assert_eq!(
        feed(b"\x1b[200~hi \x1b[A there\x1b[201~x"),
        vec![Input::Paste(Some(b"hi \x1b[A there".to_vec())), plain("x"),]
    );
    let framed = frame(r#"{"t":"quit"}"#);
    let mut pasted = b"\x1b[200~".to_vec();
    pasted.extend_from_slice(&framed);
    pasted.extend_from_slice(b"\x1b[201~");
    assert_eq!(feed(&pasted), vec![Input::Paste(Some(framed))]);
}

#[test]
fn a_paste_split_across_chunks_including_its_terminator() {
    let mut p = Parser::new();
    assert!(p.feed(b"\x1b[200~one").is_empty());
    assert!(p.feed(b" two\x1b[20").is_empty());
    assert_eq!(p.feed(b"1~"), vec![Input::Paste(Some(b"one two".to_vec()))]);
    assert!(!p.has_pending());
}

#[test]
fn an_oversized_paste_is_dropped_not_typed() {
    let mut p = Parser::new();
    assert!(p.feed(b"\x1b[200~").is_empty());
    let big = vec![b'x'; PASTE_MAX_BYTES + 1];
    assert!(p.feed(&big).is_empty());
    assert_eq!(p.feed(b"\x1b[201~"), vec![Input::Paste(None)]);
}

#[test]
fn oversized_line_is_dropped_without_key_flood() {
    let mut p = Parser::new();
    let mut big = format!("\x1eVTABS {TOKEN} {{").into_bytes();
    big.resize(PARSER_BUFFER_MAX + 2, b'x');
    assert!(p.feed(&big).is_empty());
    assert!(!p.has_pending());
    assert!(p.feed(b"rest of the same line\x1b[A").is_empty());
    assert_eq!(feed_into(&mut p, b"\n\x1b[A"), vec![plain("up")]);
}

#[test]
fn an_oversized_paste_of_keys_is_not_a_command_line() {
    let mut p = Parser::new();
    let big = vec![b'x'; PARSER_BUFFER_MAX + 2];
    assert!(p.feed(&big).is_empty());
    assert_eq!(feed_into(&mut p, b"y"), vec![plain("y")]);
}

#[test]
fn the_control_decoder_takes_a_whole_framed_record_and_nothing_else() {
    let line = frame(r#"{"t":"transport_probe","session":"inbox-42-abcd"}"#);
    assert_eq!(
        decode_control_line(&line),
        Some(control(Command::TransportProbe {
            session: "inbox-42-abcd".into(),
        }))
    );

    // non-control bytes, a key sequence and an unterminated frame are all rejected outright
    assert_eq!(decode_control_line(b"hello world\n"), None);
    assert_eq!(decode_control_line(b"\x1b[A\n"), None);
    let mut unterminated = line.clone();
    unterminated.pop();
    assert_eq!(decode_control_line(&unterminated), None);

    // a second record after the newline is not one line, so the decoder refuses the pair
    let mut two = frame(r#"{"t":"ping"}"#);
    two.extend(frame(r#"{"t":"ping"}"#));
    assert_eq!(decode_control_line(&two), None);
}
