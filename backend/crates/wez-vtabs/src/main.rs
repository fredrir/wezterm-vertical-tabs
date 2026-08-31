use vtabs_runtime::log::Logger;

/// Renders every scene JSON in `scenes` into golden-format dumps under `out`.
fn dump_frames(scenes: &str, out: &str) -> Result<(), String> {
    std::fs::create_dir_all(out).map_err(|e| format!("{out}: {e}"))?;
    let mut entries: Vec<_> = std::fs::read_dir(scenes)
        .map_err(|e| format!("{scenes}: {e}"))?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        let name = path.file_stem().unwrap().to_string_lossy().to_string();
        let raw = std::fs::read_to_string(&path).map_err(|e| format!("{name}: {e}"))?;
        let scene: vtabs_view::scene::RenderInput =
            serde_json::from_str(&raw).map_err(|e| format!("{name}: {e}"))?;
        let (text, styled) = vtabs_view::render::golden_dumps(&scene);
        std::fs::write(format!("{out}/{name}.txt"), text).map_err(|e| e.to_string())?;
        std::fs::write(format!("{out}/{name}.styled.txt"), styled).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn main() {
    // `frame` renders the Zen background and exits; it never touches the terminal or the protocol.
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("frame") => {
            if let Err(err) = vtabs_zen::frame::run(args) {
                eprintln!("{err}");
                std::process::exit(2);
            }
            return;
        }
        Some("dump-frames") => {
            let (scenes, out) = (args.next(), args.next());
            let (Some(scenes), Some(out)) = (scenes, out) else {
                eprintln!("usage: wez-vtabs dump-frames <scenes-dir> <out-dir>");
                std::process::exit(2);
            };
            if let Err(err) = dump_frames(&scenes, &out) {
                eprintln!("{err}");
                std::process::exit(2);
            }
            return;
        }
        _ => {}
    }
    if let Err(err) = vtabs_runtime::run() {
        Logger::from_env().log(format!("exit: {err}"));
        std::process::exit(1);
    }
}
