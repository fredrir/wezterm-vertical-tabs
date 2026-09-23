use crate::termwindow::TermWindowNotif;
use std::{io::Write, path::PathBuf};
use window::WindowOps;

fn write(path: PathBuf, source: &str) -> anyhow::Result<()> {
    // A dotfiles symlink keeps pointing at the rewritten file.
    let path = std::fs::canonicalize(&path).unwrap_or(path);
    let directory = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("{} has no parent directory", path.display()))?;
    std::fs::create_dir_all(directory)?;
    let mut file = tempfile::NamedTempFile::new_in(directory)?;
    file.write_all(source.as_bytes())?;
    file.as_file().sync_all()?;
    file.persist(&path)?;
    Ok(())
}

/// Atomically replace the managed Lua file, then report back to the owning window.
pub fn save(window: window::Window, path: PathBuf, source: String, mux_window_id: usize) {
    promise::spawn::spawn(async move {
        let result = smol::unblock(move || write(path, &source)).await;
        let message = match result {
            Ok(()) => serde_json::json!({"managed": {"ok": true}}),
            Err(error) => serde_json::json!({"managed": {"error": format!("{error:#}")}}),
        };
        window.notify(TermWindowNotif::Apply(Box::new(move |tw| {
            tw.vtabs_message_for(mux_window_id, message);
        })));
    })
    .detach();
}
