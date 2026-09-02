//! The wire.lua encoder and these types must agree; shapes here mirror wire.lua's builders.

use vtabs_protocol::Command;

fn parse(line: &str) -> Command {
    serde_json::from_str(line).expect(line)
}

#[test]
fn v2_lines_parse() {
    let config = r##"{"t":"config","rev":3,"rail_width":5,"position":"left",
        "icons":true,"icon_map":{"nvim":"N"},"meta":"cwd",
        "glyphs":{"custom_block":true,"east_asian_wide":false},
        "double_click_ms":300,"tear_off":true,"wheel":"scroll","context":"popover",
        "hover_timeout_ms":1200,
        "render":{"padding":{"left":1,"right":1,"top":0,"bottom":0},"frame":false,
            "tab_height":"card","row_gap":0,"separator":"gap","pinned_style":"dense",
            "close_button":"hover","show_index":false,"scroll_indicator":"auto",
            "new_tab_button":true,"new_tab_label":"New tab","hover":"follow"},
        "mac":{"integrated_buttons":false,"native_button_style":true,"preview":false,
            "is_full_screen":false}}"##;
    let Command::Config(c) = parse(config) else {
        panic!("not config")
    };
    assert_eq!(c.rev, 3);
    assert_eq!(c.render.unwrap().padding.left, 1);

    let theme = r##"{"t":"theme","rev":7,"scheme":{"background":"#1e1e2e","foreground":"#cdd6f4",
        "cursor_bg":"#f5e0dc","ansi":["#45475a","#f38ba8"]},
        "overrides":{"accent":"#89b4fa","elevation":0.12}}"##;
    let Command::Theme(t) = parse(theme) else {
        panic!("not theme")
    };
    assert_eq!(t.scheme.ansi.len(), 2);

    let model = r##"{"t":"model","rev":142,"screen":"sidebar","active":7,
        "focus":{"on":false,"index":1},"scroll":{"top":4,"user":true},
        "drag":{"id":7,"active":true,"slot":3,"outside":false,
            "origin":{"x":5,"y":6,"at":1712345678901}},
        "strip":{"buttons":[{"id":"toggle"},{"id":"open_settings"}]},
        "footer":[{"text":"main"}],
        "space":"home",
        "spaces":[{"id":"home","name":"Home","icon":"H"},{"id":"work","unseen":true}],
        "tabs":[{"id":7,"index":1,"title":"nvim","pane_title":"nvim - x","override":null,
            "proc":"nvim","cwd":"~/p/x","host":null,"user":null,"domain":"local",
            "pinned":false,"unseen":false}],"private":false}"##;
    let Command::Model(m) = parse(model) else {
        panic!("not model")
    };
    assert_eq!(m.tabs[0].proc.as_deref(), Some("nvim"));
    assert_eq!(m.drag.unwrap().origin.x, 5);
    assert_eq!(m.strip.unwrap().buttons.len(), 2);
    assert_eq!(m.space.as_deref(), Some("home"));
    assert_eq!(m.spaces.len(), 2);
    assert_eq!(m.spaces[0].icon.as_deref(), Some("H"));
    assert_eq!((m.spaces[1].name.as_str(), m.spaces[1].unseen), ("", true));

    // a model from a plugin without spaces parses with none, so the switcher never appears
    let bare = r##"{"t":"model","rev":1,"screen":"sidebar","tabs":[]}"##;
    let Command::Model(m) = parse(bare) else {
        panic!("not model")
    };
    assert!(m.space.is_none() && m.spaces.is_empty());

    let Command::Fx(fx) = parse(r##"{"t":"fx","phase":"expand"}"##) else {
        panic!()
    };
    assert_eq!(fx.phase, "expand");
    let Command::Notice(n) = parse(r##"{"t":"notice","level":"warn","text":"hi"}"##) else {
        panic!()
    };
    assert_eq!(n.text, "hi");
}
