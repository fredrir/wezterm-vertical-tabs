use super::*;
use crate::settings::value::{get_at, set_at, set_path, table};

fn path(key: &str) -> SettingPath {
    SettingPath::from_dotted(key)
}

fn field<'a>(fields: &'a [Field], key: &str) -> &'a Field {
    fields
        .iter()
        .find(|field| field.path == path(key))
        .unwrap_or_else(|| panic!("missing field {key}"))
}

fn committed(effect: DocumentEffect) -> CommitEffect {
    match effect {
        DocumentEffect::Commit(effect) => effect,
        effect => panic!("expected commit, got {effect:?}"),
    }
}

#[test]
fn builds_schema_ordered_groups_fields_widgets_and_opaque_hooks() {
    let document = SettingsDocument::new(RawSettings::default());
    let fields = document.fields();
    assert_eq!(fields[0].path, path("width"));
    assert_eq!(field(&fields, "width").widget, Widget::Stepper);
    assert_eq!(field(&fields, "width").value_text, "‹ 28 ›");
    assert_eq!(field(&fields, "position").widget, Widget::Picker);
    assert_eq!(field(&fields, "debug").widget, Widget::Toggle);
    assert_eq!(
        fields
            .iter()
            .filter(|field| field.path == path("theme.elevation"))
            .count(),
        1
    );

    let hook = field(&fields, "hooks.filter");
    assert_eq!(hook.widget, Widget::Locked);
    assert_eq!(hook.locked, Some(LockReason::Opaque));
    assert_eq!(hook.value_text, "fun()");

    assert_eq!(
        document
            .groups()
            .into_iter()
            .map(|group| group.id)
            .collect::<Vec<_>>(),
        [
            "layout",
            "cards",
            "chrome",
            "behaviour",
            "theme",
            "identity",
            "hooks",
            "backend",
            "spaces",
        ]
    );
}

#[test]
fn resets_invalid_known_values() {
    let mut raw = RawSettings::default();
    set_path(&mut raw.values, "tab_height", 3.into());
    set_path(&mut raw.values, "width", 4.into());
    let document = SettingsDocument::new(raw);
    assert_eq!(
        get_at(document.values(), &path("tab_height")),
        Some(&Value::String("card".to_owned()))
    );
    assert_eq!(
        get_at(document.values(), &path("width")),
        Some(&Value::Number(28.0))
    );
    assert_eq!(document.issues().len(), 2);
    assert!(
        document
            .issues()
            .iter()
            .any(|issue| issue.path == path("tab_height"))
    );
    assert!(
        document
            .issues()
            .iter()
            .any(|issue| issue.path == path("width"))
    );
}

#[test]
fn actions_own_activation_stepping_reset_and_commit_effects() {
    let mut document = SettingsDocument::new(RawSettings::default());
    let effect = committed(
        document
            .apply(DocumentAction::Step {
                path: path("width"),
                delta: 1,
            })
            .unwrap(),
    );
    assert_eq!(effect.path, path("width"));
    assert_eq!(effect.value, Some(Value::Number(29.0)));
    assert_eq!(effect.mode, ApplyMode::Instant);
    assert!(effect.persistence_json.contains(r#""width":29.0"#));
    assert!(effect.derived.is_empty());

    let effect = committed(
        document
            .apply(DocumentAction::Activate(path("debug")))
            .unwrap(),
    );
    assert_eq!(effect.value, Some(Value::Bool(true)));

    let effect = committed(
        document
            .apply(DocumentAction::Step {
                path: path("edge_to_edge"),
                delta: 1,
            })
            .unwrap(),
    );
    assert_eq!(effect.value, Some(Value::Bool(false)));
    assert_eq!(effect.mode, ApplyMode::Reload);

    let effect = committed(
        document
            .apply(DocumentAction::Reset(path("width")))
            .unwrap(),
    );
    assert_eq!(effect.value, Some(Value::Number(28.0)));
}

#[test]
fn cross_field_changes_stay_in_the_document_persistence_and_commit_effect() {
    let mut document = SettingsDocument::new(RawSettings::default());
    let effect = committed(
        document
            .apply(DocumentAction::Step {
                path: path("hover"),
                delta: 1,
            })
            .unwrap(),
    );
    assert_eq!(effect.value, Some(Value::String("press".into())));
    assert_eq!(
        effect.derived,
        vec![DocumentChange {
            path: path("close_button"),
            value: Some(Value::String("always".into())),
        }]
    );
    assert_eq!(
        get_at(document.values(), &path("close_button")),
        Some(&Value::String("always".into()))
    );
    assert!(
        effect
            .persistence_json
            .contains(r#""close_button":"always""#)
    );
}

#[test]
fn descriptor_policies_preserve_live_modes_and_host_ownership() {
    let host_values = BTreeSet::new();
    for (key, mode) in [
        ("width", ApplyMode::Instant),
        ("frame.margin", ApplyMode::Override),
        ("frame.border", ApplyMode::Instant),
        ("theme.split", ApplyMode::Override),
        ("edge_to_edge", ApplyMode::Reload),
        ("titlebar", ApplyMode::Reload),
        ("backend.uservar", ApplyMode::Reload),
    ] {
        assert_eq!(apply_mode(&path(key), &host_values), mode, "{key}");
    }

    let owned = BTreeSet::from(["window_padding".to_owned()]);
    assert_eq!(apply_mode(&path("frame.inset"), &owned), ApplyMode::Instant);
    assert_eq!(
        apply_mode(&path("edge_to_edge"), &owned),
        ApplyMode::Instant
    );
    assert_eq!(host_key(&path("frame.border")), None);
}

#[test]
fn editing_backspace_removes_a_complete_grapheme_and_enter_commits() {
    let mut document = SettingsDocument::new(RawSettings::default());
    assert_eq!(
        document
            .apply(DocumentAction::Activate(path("meta_sep")))
            .unwrap(),
        DocumentEffect::StateChanged
    );
    document
        .apply(DocumentAction::EditKey("👩‍👩‍👧‍👧".to_owned()))
        .unwrap();
    assert_eq!(document.state().editing.as_ref().unwrap().buffer, "  👩‍👩‍👧‍👧");
    document
        .apply(DocumentAction::EditKey("backspace".to_owned()))
        .unwrap();
    assert_eq!(document.state().editing.as_ref().unwrap().buffer, "  ");
    let effect = committed(
        document
            .apply(DocumentAction::EditKey("enter".to_owned()))
            .unwrap(),
    );
    assert_eq!(effect.value, Some(Value::String("  ".to_owned())));
    assert!(document.state().editing.is_none());
}

#[test]
fn recorder_uses_segmented_dynamic_key_paths() {
    let binding = table([("key", "o".into()), ("mods", "CTRL".into())]);
    let mut raw = RawSettings::default();
    raw.key_defaults
        .insert("open.file".to_owned(), binding.clone());
    let dynamic = SettingPath(vec!["keys".to_owned(), "open.file".to_owned()]);
    let mut document = SettingsDocument::new(raw);
    let row = document
        .fields()
        .into_iter()
        .find(|field| field.path == dynamic)
        .expect("dynamic binding row");
    assert_eq!(row.widget, Widget::Recorder);
    assert_eq!(row.default, Some(binding));

    assert_eq!(
        document
            .apply(DocumentAction::Activate(dynamic.clone()))
            .unwrap(),
        DocumentEffect::StateChanged
    );
    assert_eq!(document.state().armed, Some(dynamic.clone()));
    let effect = committed(
        document
            .apply(DocumentAction::RecordChord {
                key: "p".to_owned(),
                mods: vec!["CTRL".to_owned(), "SHIFT".to_owned()],
            })
            .unwrap(),
    );
    assert_eq!(effect.path, dynamic.clone());
    assert_eq!(
        get_at(document.values(), &dynamic),
        Some(&table([("key", "p".into()), ("mods", "CTRL|SHIFT".into())]))
    );
    assert!(document.state().armed.is_none());
}

#[test]
fn clipboard_is_valid_lua_shape_for_lists_empty_tables_and_arbitrary_keys() {
    let mut raw = RawSettings::default();
    set_path(
        &mut raw.values,
        "strip_actions",
        Value::List(vec!["search".into()]),
    );
    set_path(&mut raw.values, "frame", Value::Table(BTreeMap::new()));
    set_at(
        &mut raw.values,
        &SettingPath(vec![
            "backend".to_owned(),
            "env".to_owned(),
            "A.B-with dash".to_owned(),
        ]),
        Some("one\ntwo".into()),
    );
    let lua = SettingsDocument::new(raw).clipboard_lua();
    assert!(lua.starts_with("vtabs.apply_to_config(config, {"));
    assert!(lua.contains(r#"["A.B-with dash"] = "one\ntwo""#));
    assert!(lua.contains(r#"["frame"] = {}"#));
    assert!(lua.contains("\"search\","));
    assert!(!lua.contains("1 = \"search\""));
}

#[test]
fn persistence_omits_explicit_values_while_clipboard_includes_them() {
    let mut raw = RawSettings::default();
    set_path(&mut raw.values, "width", 42.into());
    raw.explicit.insert(path("width"));
    let document = SettingsDocument::new(raw);
    assert!(!document.persistence_json().unwrap().contains("width"));
    assert!(document.clipboard_lua().contains(r#"["width"] = 42"#));
    assert_eq!(
        field(&document.fields(), "width").locked,
        Some(LockReason::Explicit)
    );
}

#[test]
fn explicit_leaf_locks_do_not_lock_structural_siblings() {
    let mut raw = RawSettings::default();
    raw.explicit.insert(path("padding.top"));
    let document = SettingsDocument::new(raw);
    let fields = document.fields();
    assert_eq!(
        field(&fields, "padding.top").locked,
        Some(LockReason::Explicit)
    );
    assert_eq!(field(&fields, "padding.left").locked, None);
}

#[test]
fn opaque_callback_values_never_enter_persistence_or_clipboard() {
    let mut raw = RawSettings::default();
    set_path(
        &mut raw.values,
        "backend.path",
        "/serializable-projection".into(),
    );
    raw.opaque.insert(path("backend.path"));
    let document = SettingsDocument::new(raw);
    let fields = document.fields();
    let row = field(&fields, "backend.path");
    assert_eq!(row.locked, Some(LockReason::Opaque));
    assert_eq!(row.widget, Widget::Locked);
    assert_eq!(row.value_text, "fun()");
    assert!(!document.persistence_json().unwrap().contains("backend"));
    assert!(!document.clipboard_lua().contains("backend"));
}

#[test]
fn macos_recorder_caveat_only_exists_while_armed() {
    let mut raw = RawSettings {
        is_macos: true,
        ..RawSettings::default()
    };
    raw.key_defaults
        .insert("open".to_owned(), table([("key", "o".into())]));
    let mut document = SettingsDocument::new(raw);
    assert_eq!(document.caveat(), None);
    document
        .apply(DocumentAction::Activate(SettingPath(vec![
            "keys".to_owned(),
            "open".to_owned(),
        ])))
        .unwrap();
    assert_eq!(document.caveat(), Some(CAVEAT.as_slice()));
}
