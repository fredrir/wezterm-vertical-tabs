use super::*;

#[test]
fn template_configuration_survives_without_declared_spaces() {
    let mut configuration = Configuration::default();
    configuration.templates.push(core::SpaceTemplate {
        id: "host-$host".into(),
        name: "$host".into(),
        icon: "".into(),
        accent: None,
        rules: Vec::new(),
    });
    let value = configuration.value();
    assert_eq!(value["profile"], "default");
    assert_eq!(value["spaces"], json!([]));
    assert!(value["managed"].is_null());
    assert_eq!(value["templates"].as_array().unwrap().len(), 1);
}
#[test]
fn routing_type_errors_cannot_become_route_reset() {
    assert!(decode_result("routing", json!(false)).is_err());
    assert!(matches!(
        decode_result("routing", Value::Null).unwrap(),
        Some(HookResult::Route(None))
    ));
}
#[test]
fn footer_rows_are_bounded_and_normalized() {
    assert!(
        matches!(decode_result("footer", json!(["One", "Two"])).unwrap(), Some(HookResult::Footer(text)) if text == "One\nTwo")
    );
    assert!(decode_result("footer", json!(["One", 2])).is_err());
    assert!(decode_result("footer", json!(vec!["row"; 17])).is_err());
}
#[test]
fn batch_capacity_includes_window_hooks() {
    assert!(MAX_HOOK_TABS * TAB_HOOKS.len() + WINDOW_HOOKS.len() <= MAX_HOOK_RESULTS);
    assert!((MAX_HOOK_TABS + 1) * TAB_HOOKS.len() + WINDOW_HOOKS.len() > MAX_HOOK_RESULTS);
}
#[test]
fn each_lua_configuration_keeps_its_own_validated_value() {
    let a = Lua::new();
    let b = Lua::new();
    let configuration = Configuration {
        profile: "kept".into(),
        ..Configuration::default()
    };
    a.set_named_registry_value(CONFIG_REGISTRY, a.to_value(&configuration).unwrap())
        .unwrap();
    assert_eq!(configuration_from_lua(&a).unwrap().profile, "kept");
    assert_eq!(configuration_from_lua(&b).unwrap().profile, "default");
}
#[test]
fn lua_actions_are_typed_before_they_reach_a_window() {
    let lua = Lua::new();
    let action = |source: &str| -> mlua::Result<core::Action> {
        parse_action(&lua, lua.load(source).eval::<mlua::Value>()?)
    };
    let error = |source: &str| action(source).unwrap_err().to_string();
    assert_eq!(
        action("return 'navigator'").unwrap(),
        core::Action::Ui(core::UiAction::Navigator)
    );
    assert_eq!(
        action("return { SelectSpace = 'work' }").unwrap(),
        core::Action::Intent(core::Intent::SelectSpace("work".into()))
    );
    assert!(action("return 'navigate'").is_err());
    assert_eq!(
        action("return 'quick_terminal'").unwrap(),
        core::Action::Ui(core::UiAction::QuickTerminal)
    );
    assert_eq!(
        action("return { QuickTerminal = { args = { 'yazi' }, cwd = '/tmp', height = 1 } }")
            .unwrap(),
        core::Action::Ui(core::UiAction::QuickProgram(Box::new(core::QuickProgram {
            args: vec!["yazi".into()],
            cwd: Some("/tmp".into()),
            height: Some(1.),
            ..Default::default()
        })))
    );
    assert_eq!(
        action("return { QuickTerminal = {} }").unwrap(),
        core::Action::Ui(core::UiAction::QuickProgram(Default::default()))
    );
    for invalid in [
        "{ width = 0 }",
        "{ height = 1.5 }",
        "{ args = { 'lazygit' }, size = 0.5 }",
    ] {
        assert!(action(&format!("return {{ QuickTerminal = {invalid} }}")).is_err());
    }
    assert!(
        error("return { QuickTerminal = { width = 100 } }")
            .contains("invalid vtabs action: size must be above 0 and at most 1")
    );
    assert!(error("return 'navigate'").contains("unknown action \"navigate\""));
    assert!(error("return { SelectSpace = 5 }").contains("expected a string"));
}
#[test]
fn quick_programs_read_wezterm_values() {
    use crate::termwindow::ui_host::{
        HideOnEscape, TerminalBorder, TerminalPadding, TerminalProgram,
    };
    use config::Dimension;
    let lua = Lua::new();
    let program = |fields: &str| {
        let source = format!("return {{ QuickTerminal = {{ {fields} }} }}");
        let value = lua.load(source).eval::<mlua::Value>().unwrap();
        match parse_action(&lua, value).map_err(|error| error.to_string())? {
            core::Action::Ui(core::UiAction::QuickProgram(program)) => {
                super::super::terminal_program::quick_program(*program)
            }
            other => Err(format!("{other:?}")),
        }
    };
    let lazygit = program(
        "args = { 'lazygit' }, domain = { DomainName = 'local' }, preserve = true, \
         window_padding = { left = 0, top = '0.5cell', right = '10%', bottom = '6pt' }, \
         window_background_opacity = 0.9, blur = 20, border_color = 'none', \
         auto_hide = true, hide_on_escape = 'never', colors = { background = '#1e1e2e', \
         foreground = 'purple', ansi = { 'black', 'maroon', 'green', 'olive', 'navy', \
         'purple', 'teal', 'silver' }, indexed = { [136] = '#af8700' } }",
    )
    .unwrap();
    assert_eq!(
        lazygit.spawn.domain,
        config::keyassignment::SpawnTabDomain::DomainName("local".into())
    );
    assert!(lazygit.preserve);
    assert_eq!(
        lazygit.padding,
        TerminalPadding {
            left: Some(Dimension::Pixels(0.)),
            top: Some(Dimension::Cells(0.5)),
            right: Some(Dimension::Percent(0.1)),
            bottom: Some(Dimension::Points(6.)),
        }
    );
    assert_eq!((lazygit.opacity, lazygit.blur), (Some(0.9), Some(20.)));
    assert_eq!(lazygit.border, Some(TerminalBorder::None));
    assert_eq!(
        (lazygit.auto_hide, lazygit.hide_on_escape),
        (Some(true), Some(HideOnEscape::Never))
    );
    let colors = lazygit.colors.unwrap();
    assert_eq!(colors.background, Some((0x1e, 0x1e, 0x2e).into()));
    assert!(colors.foreground.is_some() && colors.ansi.is_some());
    assert!(colors.indexed.contains_key(&136));

    let defaults = program("").unwrap();
    assert_eq!(defaults, TerminalProgram::default());
    assert_eq!(
        program("border_color = 'rgb(255, 0, 0)'").unwrap().border,
        Some(TerminalBorder::Color(window::color::LinearRgba(
            1., 0., 0., 1.
        )))
    );
    struct Shade;
    impl mlua::UserData for Shade {
        fn add_methods<'lua, M: mlua::UserDataMethods<'lua, Self>>(methods: &mut M) {
            methods.add_meta_method(mlua::MetaMethod::ToString, |_, _, ()| Ok("#ff0000"));
        }
    }
    lua.globals().set("shade", Shade).unwrap();
    let objects = program("border_color = shade, colors = { background = shade } ").unwrap();
    assert_eq!(
        objects.border,
        Some(TerminalBorder::Color(window::color::LinearRgba(
            1., 0., 0., 1.
        )))
    );
    assert_eq!(
        objects.colors.and_then(|colors| colors.background),
        Some((255, 0, 0).into())
    );
    let recursive = lua
        .load("local t = { QuickTerminal = {} }; t.QuickTerminal.colors = t; return t")
        .eval::<mlua::Value>()
        .unwrap();
    assert!(parse_action(&lua, recursive).is_err());
    for (fields, error) in [
        ("colors = { background = 'nope' }", "invalid colors"),
        ("colors = { backgroud = '#000000' }", "invalid colors"),
        ("border_color = 'nope'", "invalid border_color nope"),
        ("blur = 65", "blur must be at most 64"),
        ("window_background_opacity = 2", "window_background_opacity"),
        ("window_padding = { left = '1em' }", "padding takes pixels"),
        ("window_padding = { middle = 1 }", "unknown field"),
        ("domain = 'Elsewhere'", "unknown variant `Elsewhere`"),
        (
            "hide_on_escape = 'sometimes'",
            "unknown variant `sometimes`",
        ),
        ("preserve = 'yes'", "invalid type"),
    ] {
        let found = program(fields).unwrap_err();
        assert!(found.contains(error), "{}: {}", fields, found);
    }
}
#[test]
fn managed_file_tables_round_trip_through_the_registry() {
    let lua = Lua::new();
    let table = lua
        .load(
            "return { managed = { settings = { width = 300 }, spaces = {} }, \
             managed_path = '/tmp/vtabs_settings.lua' }",
        )
        .eval::<mlua::Value>()
        .unwrap();
    let configuration: Configuration = lua.from_value(table).unwrap();
    lua.set_named_registry_value(CONFIG_REGISTRY, lua.to_value(&configuration).unwrap())
        .unwrap();
    let restored = configuration_from_lua(&lua).unwrap();
    assert_eq!(restored.managed.unwrap().settings["width"], json!(300));
    assert_eq!(
        restored.managed_path.unwrap(),
        std::path::Path::new("/tmp/vtabs_settings.lua")
    );
}

#[test]
fn saved_keybinds_round_trip_through_lua_including_unbound_actions() {
    let lua = Lua::new();
    let mut app = WindowApp::default();
    app.config(json!({"managed": {}})).unwrap();
    app.dispatch(core::Intent::SetSetting {
        key: "keybinds".into(),
        value: json!({"commands": ["F12"], "refresh": []}),
    })
    .unwrap();
    let source = app.take_managed_write().unwrap();
    let managed = lua.load(&source).eval::<mlua::Value>().unwrap();
    let table = lua.create_table().unwrap();
    table.set("managed", managed).unwrap();
    let config: Configuration = lua.from_value(mlua::Value::Table(table)).unwrap();
    let mut restored = WindowApp::default();
    restored.config(config.value()).unwrap();
    assert_eq!(
        core::keybinds::action(&restored.model().settings, "F12"),
        Some("commands")
    );
    assert!(core::keybinds::bindings(&restored.model().settings, "refresh").is_empty());
}
