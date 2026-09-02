use std::collections::BTreeMap;

use vtabs_engine::interaction::{SettingsScreen, settings_key, settings_mouse};
use vtabs_engine::settings::document::Widget;
use vtabs_engine::settings::presentation::{
    PresentationField, PresentationGroup, PresentationLock, SettingsPresentation,
};
use vtabs_engine::settings::value::SettingPath;
use vtabs_engine::settings::{self, SettingsView};
use vtabs_engine::theme::{Palette, Theme, UserTheme};
use vtabs_engine::ui::SettingsUi;
use vtabs_protocol::types::{Button, Mods, Mouse, MouseKind};
use vtabs_protocol::{Event, Intent, Modifier};

const DIMS: (i64, i64) = (100, 21);

fn theme() -> Theme {
    vtabs_engine::theme::resolve(&UserTheme::default(), &Palette::default(), false)
}

/// Two groups, four fields: a stepper, a picker, a locked toggle and a text row.
fn model() -> SettingsPresentation {
    fn field(key: &str, group: &str, widget: Widget, value_text: &str) -> PresentationField {
        PresentationField {
            path: SettingPath::from_dotted(key),
            label: key.to_owned(),
            group: group.to_owned(),
            widget,
            value_text: value_text.to_owned(),
            changed: false,
            locked: None,
            depth: 0,
            help: key.to_owned(),
            editing: None,
            armed: false,
        }
    }

    let mut locked = field("dim_inactive_panes", "layout", Widget::Toggle, "[ off ]");
    locked.locked = Some(PresentationLock {
        text: "wezterm.lua (host)".to_owned(),
    });
    SettingsPresentation {
        fields: vec![
            field("width", "layout", Widget::Stepper, "< 28 >"),
            field("position", "layout", Widget::Picker, "< left >"),
            locked,
            field("meta_sep", "cards", Widget::Text, "\"  \""),
        ],
        groups: vec![
            PresentationGroup {
                id: "layout".to_owned(),
                label: "Layout".to_owned(),
            },
            PresentationGroup {
                id: "cards".to_owned(),
                label: "Cards".to_owned(),
            },
        ],
        caveat: None,
        version: Some("9.9.9".to_owned()),
    }
}

fn view<'a>(model: &'a SettingsPresentation, ui: &'a SettingsUi) -> SettingsView<'a> {
    SettingsView {
        cols: DIMS.0,
        rows: DIMS.1,
        presentation: model,
        ui,
        theme: theme(),
        glyphs: BTreeMap::new(),
    }
}

fn press(x: u16, y: u16) -> Mouse {
    Mouse {
        kind: MouseKind::Press,
        button: Button::Left,
        x,
        y,
        dy: 0,
        mods: Mods::default(),
    }
}

/// One key against a model and a state, returning the state it left and the events it sent.
fn key(model: &SettingsPresentation, ui: &SettingsUi, name: &str) -> (SettingsUi, Vec<Event>) {
    key_with(model, ui, name, Mods::default())
}

fn key_with(
    model: &SettingsPresentation,
    ui: &SettingsUi,
    name: &str,
    mods: Mods,
) -> (SettingsUi, Vec<Event>) {
    let view = view(model, ui);
    let plan = settings::plan(&view);
    let screen = SettingsScreen {
        plan: &plan,
        editing: view.editing(),
        armed: view.armed(),
    };
    let r = settings_key(&screen, ui, name, mods);
    (r.settings.expect("settings state"), r.events)
}

fn click(
    model: &SettingsPresentation,
    ui: &SettingsUi,
    x: u16,
    y: u16,
) -> (SettingsUi, Vec<Event>) {
    let view = view(model, ui);
    let plan = settings::plan(&view);
    let screen = SettingsScreen {
        plan: &plan,
        editing: view.editing(),
        armed: view.armed(),
    };
    let r = settings_mouse(&screen, ui, &press(x, y));
    (r.settings.expect("settings state"), r.events)
}

fn verbs(events: &[Event]) -> Vec<(&str, Option<String>, Option<i64>)> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::Intent { intent } => {
                let (key, delta) = match intent {
                    Intent::NudgeOption { key, delta } => (Some(key.clone()), Some(*delta)),
                    Intent::ActivateOption { key }
                    | Intent::ResetOption { key }
                    | Intent::EditKey { key }
                    | Intent::RecordChord { key, .. } => (Some(key.clone()), None),
                    _ => (None, None),
                };
                Some((intent.name(), key, delta))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn nav_moves_the_focus_and_never_leaves_the_group() {
    let m = model();
    let ui = SettingsUi::default();
    let (down, events) = key(&m, &ui, "j");
    assert_eq!(down.focus, 2);
    assert!(events.is_empty(), "navigation never crosses the wire");
    assert_eq!(key(&m, &down, "down").0.focus, 3);
    // three rows in `layout`, so the end is a stop, not a wrap
    let end = SettingsUi {
        focus: 3,
        ..ui.clone()
    };
    assert_eq!(key(&m, &end, "j").0.focus, 3);
    assert_eq!(key(&m, &ui, "k").0.focus, 1, "and the start is too");
}

#[test]
fn tab_cycles_the_groups_and_resets_the_form() {
    let m = model();
    let ui = SettingsUi {
        focus: 3,
        scroll: 2,
        ..Default::default()
    };
    let (next, _) = key(&m, &ui, "tab");
    assert_eq!((next.group, next.focus, next.scroll), (2, 1, 0));
    assert_eq!(key(&m, &next, "tab").0.group, 1, "two groups, so it wraps");
}

#[test]
fn the_filter_narrows_the_rows_the_focus_can_reach() {
    let m = model();
    let ui = SettingsUi::default();
    let (open, events) = key(&m, &ui, "/");
    assert!(open.filtering && open.filter.is_empty() && open.focus == 1);
    assert!(events.is_empty());

    let (typed, _) = key(&m, &open, "p");
    let (typed, _) = key(&m, &typed, "o");
    assert_eq!(typed.filter, "po");
    // `position` is the only layout key holding "po", so j cannot move off it
    let (committed, _) = key(&m, &typed, "enter");
    assert!(
        !committed.filtering,
        "enter keeps the filter and leaves the mode"
    );
    assert_eq!(key(&m, &committed, "j").0.focus, 1);

    let (cleared, _) = key(&m, &typed, "escape");
    assert!(!cleared.filtering && cleared.filter.is_empty());
    assert_eq!(key(&m, &cleared, "j").0.focus, 2, "the whole group is back");
}

#[test]
fn the_verbs_name_the_focused_key_and_never_predict_the_commit() {
    let m = model();
    let ui = SettingsUi::default();
    assert_eq!(
        verbs(&key(&m, &ui, "right").1),
        vec![("nudge_option", Some("width".into()), Some(1))]
    );
    assert_eq!(
        verbs(&key(&m, &ui, "left").1),
        vec![("nudge_option", Some("width".into()), Some(-1))]
    );
    assert_eq!(
        verbs(&key(&m, &ui, "enter").1),
        vec![("activate_option", Some("width".into()), None)]
    );
    assert_eq!(
        verbs(&key(&m, &ui, "space").1),
        vec![("activate_option", Some("width".into()), None)]
    );
    assert_eq!(
        verbs(&key(&m, &ui, "r").1),
        vec![("reset_option", Some("width".into()), None)]
    );
    assert_eq!(
        verbs(&key(&m, &ui, "c").1),
        vec![("settings_copy", None, None)]
    );
}

#[test]
fn a_locked_row_is_consumed_and_left_alone() {
    let m = model();
    let locked = SettingsUi {
        focus: 3,
        ..Default::default()
    };
    for name in ["right", "left", "enter", "r"] {
        assert!(
            key(&m, &locked, name).1.is_empty(),
            "{name} must not act on a locked row"
        );
    }
}

#[test]
fn escape_and_bare_q_close_the_page_but_only_outside_a_modal_mode() {
    let m = model();
    let ui = SettingsUi::default();
    assert_eq!(
        verbs(&key(&m, &ui, "escape").1),
        vec![("close_settings", None, None)]
    );
    assert_eq!(
        verbs(&key(&m, &ui, "q").1),
        vec![("close_settings", None, None)]
    );
    let ctrl = Mods {
        ctrl: true,
        ..Default::default()
    };
    assert!(
        key_with(&m, &ui, "q", ctrl).1.is_empty(),
        "ctrl+q is not bare"
    );

    let mut editing = model();
    editing.fields[0].editing = Some("28".into());
    assert_eq!(
        verbs(&key(&editing, &ui, "escape").1),
        vec![("edit_key", Some("escape".into()), None)],
        "escape unwinds the edit, not the page"
    );
    assert_eq!(
        verbs(&key(&editing, &ui, "q").1),
        vec![("edit_key", Some("q".into()), None)]
    );

    let filtering = SettingsUi {
        filtering: true,
        ..Default::default()
    };
    assert!(
        key(&m, &filtering, "q").1.is_empty(),
        "q is a filter character"
    );
    assert!(
        !key(&m, &filtering, "escape").0.filtering,
        "escape leaves the filter"
    );

    let mut armed = model();
    armed.fields[0].armed = true;
    assert_eq!(
        verbs(&key(&armed, &ui, "escape").1),
        vec![("record_chord", Some("escape".into()), None)],
        "the recorder sees escape; SettingsDocument consumes it as cancel"
    );
}

#[test]
fn an_armed_recorder_takes_whatever_the_pty_delivered() {
    let mut m = model();
    m.fields[0].armed = true;
    let ui = SettingsUi::default();
    let (_, events) = key_with(
        &m,
        &ui,
        "p",
        Mods {
            ctrl: true,
            shift: true,
            ..Default::default()
        },
    );
    match events.as_slice() {
        [
            Event::Intent {
                intent: Intent::RecordChord { key, mods },
            },
        ] => {
            assert_eq!(key, "p");
            assert_eq!(*mods, vec![Modifier::Shift, Modifier::Ctrl]);
        }
        other => panic!("expected one record_chord, got {other:?}"),
    }
}

#[test]
fn an_unhandled_key_is_swallowed_rather_than_forwarded() {
    let m = model();
    let ui = SettingsUi::default();
    let (after, events) = key(&m, &ui, "z");
    assert!(
        events.is_empty(),
        "the page owns the keyboard while it is up"
    );
    assert_eq!(after, ui, "and nothing moved");
}

#[test]
fn a_click_is_routed_by_the_column_it_landed_in() {
    let m = model();
    let ui = SettingsUi::default();
    let g = settings::grid(DIMS.0).expect("a grid at 100 columns");
    // the form's first body row is row 4; `position` is the second
    let row = 5u16;

    let (nav, events) = click(&m, &ui, g.nav_x1 as u16, 5);
    assert_eq!(
        (nav.group, nav.focus, nav.scroll),
        (2, 1, 0),
        "row 5 is the Cards nav"
    );
    assert!(events.is_empty(), "the nav never crosses either");

    let (dec, events) = click(&m, &ui, (g.value_x2 - 11) as u16, row);
    assert_eq!(dec.focus, 2, "the click focuses what it hit");
    assert_eq!(
        verbs(&events),
        vec![("nudge_option", Some("position".into()), Some(-1))]
    );
    assert_eq!(
        verbs(&click(&m, &ui, g.value_x2 as u16, row).1),
        vec![("nudge_option", Some("position".into()), Some(1))]
    );
    // between the two steppers is the value column, which Enter's verb answers for
    assert_eq!(
        verbs(&click(&m, &ui, (g.value_x2 - 5) as u16, row).1),
        vec![("activate_option", Some("position".into()), None)]
    );
    // the label column only takes the focus
    let (field, events) = click(&m, &ui, g.label_x as u16, row);
    assert_eq!(field.focus, 2);
    assert!(events.is_empty());
}

#[test]
fn a_locked_row_offers_no_stepper_to_click() {
    let m = model();
    let ui = SettingsUi::default();
    let g = settings::grid(DIMS.0).expect("a grid at 100 columns");
    // row 6 is `dim_inactive_panes`, locked: dec/inc are not planned, so the value span wins
    let (focus, events) = click(&m, &ui, g.value_x2 as u16, 6);
    assert_eq!(focus.focus, 3);
    assert!(events.is_empty(), "and activate refuses a locked row");
}

#[test]
fn a_click_off_every_span_changes_nothing() {
    let m = model();
    let ui = SettingsUi::default();
    let (after, events) = click(&m, &ui, 1, 1);
    assert_eq!(after, ui);
    assert!(events.is_empty());
}
