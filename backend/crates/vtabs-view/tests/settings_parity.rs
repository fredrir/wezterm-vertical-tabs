//! Every committed settings scene must render byte-identically to its golden dumps — the gate
//! 07-p6-spec.md puts in front of retiring page.lua's paint path.
//! Scenes: plugin/tests/golden/scenes/settings-*.json (exported by the Lua harness).
//! Goldens: plugin/tests/golden/frames/settings-*.txt + .styled.txt.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::Deserialize;
use vtabs_core::ui::SettingsUi;
use vtabs_protocol::v2::ModelMsg;
use vtabs_view::settings::{SettingsView, golden_dumps};

#[derive(Deserialize)]
struct Scene {
    cols: i64,
    rows: i64,
    theme: vtabs_theme::Theme,
    glyphs: BTreeMap<String, String>,
    #[serde(default)]
    ui: SettingsUi,
    /// The scene stores the model body; `wire.versioned` stamps `rev` on it at send time.
    model: serde_json::Map<String, serde_json::Value>,
}

fn model_of(scene: &Scene) -> ModelMsg {
    let mut body = scene.model.clone();
    body.insert("rev".into(), serde_json::json!(1));
    serde_json::from_value(serde_json::Value::Object(body)).expect("model body")
}

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../plugin/tests/golden")
}

#[test]
fn every_settings_scene_matches_its_goldens() {
    let scenes = golden_dir().join("scenes");
    let frames = golden_dir().join("frames");
    let mut names: Vec<String> = std::fs::read_dir(&scenes)
        .expect("scenes dir")
        .filter_map(|e| {
            let name = e.ok()?.file_name().into_string().ok()?;
            name.strip_suffix(".json").map(str::to_string)
        })
        .filter(|name| name.starts_with("settings-"))
        .collect();
    names.sort();
    assert_eq!(
        names.len(),
        4,
        "expected settings-100/60/40/pending, found {names:?}"
    );
    let mut failures = Vec::new();
    for name in &names {
        let raw = std::fs::read_to_string(scenes.join(format!("{name}.json"))).unwrap();
        let scene: Scene =
            serde_json::from_str(&raw).unwrap_or_else(|e| panic!("{name}.json: {e}"));
        let model = model_of(&scene);
        let view = SettingsView {
            cols: scene.cols,
            rows: scene.rows,
            model: &model,
            ui: &scene.ui,
            theme: scene.theme.clone(),
            glyphs: scene.glyphs.clone(),
            position: "left".into(),
            meta_sep: None,
        };
        let (text, styled) = golden_dumps(&view);
        let want_text = std::fs::read_to_string(frames.join(format!("{name}.txt"))).unwrap();
        let want_styled =
            std::fs::read_to_string(frames.join(format!("{name}.styled.txt"))).unwrap();
        if text != want_text {
            failures.push(format!("{name}.txt:\n--- want\n{want_text}--- got\n{text}"));
        }
        if styled != want_styled {
            failures.push(format!(
                "{name}.styled.txt:\n--- want\n{want_styled}--- got\n{styled}"
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} mismatches:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
