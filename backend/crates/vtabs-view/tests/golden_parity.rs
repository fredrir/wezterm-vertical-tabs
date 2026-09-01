//! Every committed sidebar scene must render byte-identically to its golden dumps.
//! Scenes: plugin/tests/golden/scenes/*.json (exported by the Lua harness).
//! Goldens: plugin/tests/golden/frames/<scene>.txt + .styled.txt.

use std::path::PathBuf;

use vtabs_view::render::golden_dumps;
use vtabs_view::scene::RenderInput;

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../plugin/tests/golden")
}

#[test]
fn every_scene_matches_its_goldens() {
    let scenes = golden_dir().join("scenes");
    let frames = golden_dir().join("frames");
    let mut names: Vec<String> = std::fs::read_dir(&scenes)
        .expect("scenes dir")
        .filter_map(|e| {
            let name = e.ok()?.file_name().into_string().ok()?;
            name.strip_suffix(".json").map(str::to_string)
        })
        // settings-* scenes are the settings screen's; its own widget test gates them
        .filter(|name| !name.starts_with("settings-"))
        .collect();
    names.sort();
    assert!(
        names.len() >= 17,
        "expected the 17 sidebar scenes, found {}",
        names.len()
    );
    let mut failures = Vec::new();
    for name in &names {
        let raw = std::fs::read_to_string(scenes.join(format!("{name}.json"))).unwrap();
        let input: RenderInput =
            serde_json::from_str(&raw).unwrap_or_else(|e| panic!("{name}.json: {e}"));
        let (text, styled) = golden_dumps(&input);
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
