use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, ensure};
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

use crate::state::{self, BuildMetadata};

pub const SOURCE_ITEMS: &[&str] = &[
    "Cargo.toml",
    "Cargo.lock",
    "rust-toolchain",
    "rust-toolchain.toml",
    "pyproject.toml",
    "uv.lock",
    "ruff.toml",
    "rustfmt.toml",
    "stylua.toml",
    ".editorconfig",
    ".cargo",
    ".github",
    "README.md",
    "justfile",
    "src",
    "wezterm",
    "plugin",
    "docs",
    "tools",
    "tests",
];
pub const IGNORED: &[&str] = &[
    "target",
    "__pycache__",
    ".git",
    ".pytest_cache",
    ".ruff_cache",
    ".mypy_cache",
    ".venv",
    "node_modules",
];

pub fn is_ignored(relative: &Path) -> bool {
    relative
        .components()
        .any(|part| IGNORED.iter().any(|ignored| part.as_os_str() == *ignored))
}

pub(super) fn walk_files(path: &Path, files: &mut BTreeSet<PathBuf>) -> Result<()> {
    for entry in WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| !IGNORED.iter().any(|ignored| entry.file_name() == *ignored))
    {
        let entry = entry.with_context(|| format!("read source {}", path.display()))?;
        ensure!(
            !entry.file_type().is_symlink(),
            "source symlinks are unsupported: {}",
            entry.path().display()
        );
        if entry.file_type().is_file() {
            files.insert(entry.into_path());
        }
    }
    Ok(())
}

pub fn source_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut files = BTreeSet::new();
    for item in SOURCE_ITEMS {
        let path = root.join(item);
        if !path.exists() {
            continue;
        }
        walk_files(&path, &mut files)?;
    }
    Ok(files.into_iter().collect())
}

pub(super) fn digest_entry(digest: &mut Sha256, relative: &str, contents: &[u8]) {
    digest.update(relative.as_bytes());
    digest.update([0]);
    digest.update(contents);
    digest.update([0]);
}

pub(super) fn digest_paths<'a>(
    root: &Path,
    files: impl IntoIterator<Item = &'a PathBuf>,
) -> Result<String> {
    let mut digest = Sha256::new();
    for path in files {
        let relative = path
            .strip_prefix(root)?
            .to_string_lossy()
            .replace('\\', "/");
        let contents = fs::read(path).with_context(|| format!("read {}", path.display()))?;
        digest_entry(&mut digest, &relative, &contents);
    }
    Ok(state::hex(digest.finalize()))
}

pub fn source_digest(root: &Path) -> Result<String> {
    digest_paths(root, &source_files(root)?)
}

pub fn is_compile_input(relative: &Path) -> bool {
    matches!(
        relative.to_str(),
        Some("Cargo.toml" | "Cargo.lock" | "rust-toolchain" | "rust-toolchain.toml")
    ) || relative.starts_with(".cargo")
        || relative.starts_with("wezterm")
        || (relative.starts_with("src")
            && !relative.components().any(|part| {
                matches!(
                    part.as_os_str().to_str(),
                    Some("tests" | "benches" | "examples")
                )
            })
            && !matches!(
                relative.extension().and_then(|v| v.to_str()),
                Some("md" | "rst")
            ))
}

fn filtered_digest(root: &Path, include: impl Fn(&Path) -> bool) -> Result<String> {
    let files = source_files(root)?;
    digest_paths(
        root,
        files.iter().filter(|path| {
            include(
                path.strip_prefix(root)
                    .expect("source file belongs to root"),
            )
        }),
    )
}

pub fn compile_digest(root: &Path) -> Result<String> {
    filtered_digest(root, is_compile_input)
}

pub fn validation_digest(root: &Path) -> Result<String> {
    filtered_digest(root, |relative| {
        ["src", "wezterm", "tests", "plugin", "tools", ".cargo"]
            .iter()
            .any(|directory| relative.starts_with(directory))
            || matches!(
                relative.to_str(),
                Some(
                    "Cargo.toml"
                        | "Cargo.lock"
                        | "rust-toolchain"
                        | "rust-toolchain.toml"
                        | "pyproject.toml"
                        | "uv.lock"
                )
            )
    })
}

pub fn copy_source(root: &Path, destination: &Path) -> Result<()> {
    ensure!(
        !destination.starts_with(root.join("tools")),
        "source snapshot must be outside tools/"
    );
    for source in source_files(root)? {
        let target = destination.join(source.strip_prefix(root)?);
        fs::create_dir_all(target.parent().context("source file has no parent")?)?;
        fs::copy(source, target)?;
    }
    Ok(())
}

pub fn verify_source(root: &Path, metadata: &BuildMetadata) -> Result<()> {
    ensure!(
        source_digest(root)? == metadata.source_digest,
        "project source changed during build or packaging; run again"
    );
    Ok(())
}
