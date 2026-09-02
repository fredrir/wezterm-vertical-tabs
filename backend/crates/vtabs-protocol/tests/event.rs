use vtabs_protocol::{Event, Mods};

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
        Event::do_named("strip", "settings".into()).to_json(),
        r#"{"t":"do","a":"strip","id":"settings"}"#
    );
    assert_eq!(
        Event::do_named("switch_space", "work".into()).to_json(),
        r#"{"t":"do","a":"switch_space","id":"work"}"#
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
