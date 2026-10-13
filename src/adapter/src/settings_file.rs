use super::{Message, WindowEndpoint};
use std::{io::Write, path::PathBuf};

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
pub fn save(endpoint: WindowEndpoint, path: PathBuf, source: String) {
    promise::spawn::spawn(async move {
        let result = smol::unblock(move || write(path, &source)).await;
        let message = Message::Managed(result.map_err(|error| format!("{error:#}")));
        endpoint.send(message);
    })
    .detach();
}
