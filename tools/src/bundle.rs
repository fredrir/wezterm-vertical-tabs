//! Immutable runtime bundles and their portable integrity manifests.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context as _, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

use crate::process::CommandSpec;
use crate::state::{self, BuildMetadata, Context};

const MANIFEST: &str = "checksums.json";
const BINARIES: &[&str] = &[
    "wezterm-gui",
    "wezterm",
    "wezterm-mux-server",
    "strip-ansi-escapes",
    "wez-vtabs-store",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FileDigest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symlink: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BundleManifest {
    pub schema_version: u32,
    pub id: String,
    pub target: String,
    pub source_digest: String,
    pub files: BTreeMap<String, FileDigest>,
}

pub fn executable_name(name: &str) -> String {
    format!("{name}{}", std::env::consts::EXE_SUFFIX)
}

pub fn binary_dir(bundle: &Path) -> PathBuf {
    if cfg!(target_os = "macos") {
        bundle.join("WezTerm.app/Contents/MacOS")
    } else if cfg!(windows) {
        bundle.to_path_buf()
    } else {
        bundle.join("bin")
    }
}

pub fn tool_path(bundle: &Path) -> PathBuf {
    binary_dir(bundle).join(executable_name("wez-vtabs"))
}

pub fn hash_file(path: &Path) -> Result<String> {
    let mut input = File::open(path).with_context(|| format!("reading {}", path.display()))?;
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = input.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

pub fn safe_relative(path: &Path) -> Result<()> {
    ensure!(
        !path.as_os_str().is_empty()
            && path
                .components()
                .all(|part| matches!(part, Component::Normal(_))),
        "unsafe bundle path: {}",
        path.display()
    );
    ensure!(
        !path.to_string_lossy().contains('\\'),
        "nonportable bundle path: {}",
        path.display()
    );
    Ok(())
}

fn validate_link(root: &Path, path: &Path, target: &Path) -> Result<()> {
    ensure!(
        !target.is_absolute(),
        "absolute bundle symlink: {}",
        path.display()
    );
    let relative = path.strip_prefix(root)?;
    let mut depth = relative
        .parent()
        .map_or(0, |parent| parent.components().count());
    for component in target.components() {
        match component {
            Component::Normal(_) => depth += 1,
            Component::CurDir => {}
            Component::ParentDir => {
                ensure!(depth > 0, "bundle symlink escapes root: {}", path.display());
                depth -= 1;
            }
            _ => bail!("invalid bundle symlink: {}", path.display()),
        }
    }
    Ok(())
}

fn inventory(root: &Path) -> Result<BTreeMap<String, FileDigest>> {
    let mut files = BTreeMap::new();
    for entry in WalkDir::new(root)
        .follow_links(false)
        .sort_by_file_name()
        .min_depth(1)
    {
        let entry = entry?;
        if entry.file_type().is_dir() {
            continue;
        }
        let relative = entry.path().strip_prefix(root)?;
        safe_relative(relative)?;
        if relative == Path::new(MANIFEST) {
            continue;
        }
        let mode = {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                Some(fs::symlink_metadata(entry.path())?.permissions().mode() & 0o777)
            }
            #[cfg(not(unix))]
            {
                None
            }
        };
        let digest = if entry.file_type().is_symlink() {
            let target = fs::read_link(entry.path())?;
            validate_link(root, entry.path(), &target)?;
            ensure!(
                entry
                    .path()
                    .canonicalize()?
                    .starts_with(root.canonicalize()?),
                "bundle symlink resolves outside root: {}",
                entry.path().display()
            );
            FileDigest {
                sha256: None,
                symlink: Some(target.to_string_lossy().into_owned()),
                mode,
            }
        } else {
            ensure!(entry.file_type().is_file(), "unsupported bundle file type");
            FileDigest {
                sha256: Some(hash_file(entry.path())?),
                symlink: None,
                mode,
            }
        };
        files.insert(relative.to_string_lossy().replace('\\', "/"), digest);
    }
    Ok(files)
}

fn metadata(bundle: &Path) -> Result<BuildMetadata> {
    let metadata: BuildMetadata =
        state::read_json(&bundle.join("build.json"))?.context("native bundle metadata missing")?;
    ensure!(
        metadata.capability == 1,
        "native bundle metadata incompatible"
    );
    state::safe_id(&metadata.id)?;
    ensure!(
        metadata.target == env!("WEZ_VTABS_TARGET"),
        "bundle target mismatch: expected {}, found {}",
        env!("WEZ_VTABS_TARGET"),
        metadata.target
    );
    Ok(metadata)
}

pub fn write_manifest(bundle: &Path) -> Result<serde_json::Value> {
    let metadata = metadata(bundle)?;
    let manifest = BundleManifest {
        schema_version: 1,
        id: metadata.id,
        target: metadata.target,
        source_digest: metadata.source_digest,
        files: inventory(bundle)?,
    };
    state::write_json(&bundle.join(MANIFEST), &manifest)?;
    Ok(serde_json::to_value(manifest)?)
}

pub fn verify(bundle: &Path) -> Result<BuildMetadata> {
    let metadata = metadata(bundle)?;
    let manifest: BundleManifest = state::read_json(&bundle.join(MANIFEST))?
        .context("bundle checksums missing; rebuild or obtain a verified bundle")?;
    ensure!(
        manifest.schema_version == 1,
        "unsupported checksum manifest version"
    );
    ensure!(
        manifest.id == metadata.id
            && manifest.target == metadata.target
            && manifest.source_digest == metadata.source_digest,
        "bundle checksum identity mismatch"
    );
    let actual = inventory(bundle)?;
    ensure!(
        actual == manifest.files,
        "bundle checksum mismatch: contents or permissions changed"
    );
    for required in [
        crate::install::gui_path(bundle),
        binary_dir(bundle).join(executable_name("wez-vtabs-store")),
        tool_path(bundle),
        bundle.join("source/Cargo.toml"),
        bundle.join("source/tools/Cargo.toml"),
    ] {
        ensure!(
            required.is_file(),
            "native bundle incomplete: {}",
            required.display()
        );
    }
    Ok(metadata)
}

pub fn copy_tree(source: &Path, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination)?;
    for entry in WalkDir::new(source)
        .follow_links(false)
        .sort_by_file_name()
        .min_depth(1)
    {
        let entry = entry?;
        let target = destination.join(entry.path().strip_prefix(source)?);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target)?;
            fs::set_permissions(&target, fs::metadata(entry.path())?.permissions())?;
        } else if entry.file_type().is_symlink() {
            let link = fs::read_link(entry.path())?;
            validate_link(source, entry.path(), &link)?;
            #[cfg(unix)]
            std::os::unix::fs::symlink(link, &target)?;
            #[cfg(windows)]
            if entry.path().is_dir() {
                std::os::windows::fs::symlink_dir(link, &target)?;
            } else {
                std::os::windows::fs::symlink_file(link, &target)?;
            }
        } else {
            ensure!(entry.file_type().is_file(), "unsupported source file type");
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

fn copy_file(source: &Path, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination.parent().context("file parent missing")?)?;
    fs::copy(source, destination)
        .with_context(|| format!("copy {} to {}", source.display(), destination.display()))?;
    Ok(())
}

fn shell_assets(source: &Path, bundle: &Path, resources: &Path) -> Result<()> {
    let assets = source.join("assets");
    if cfg!(target_os = "macos") {
        copy_tree(&assets.join("shell-integration"), resources)?;
        copy_tree(
            &assets.join("shell-completion"),
            &resources.join("shell-completion"),
        )?;
    } else if cfg!(target_os = "linux") {
        for (original, relative) in [
            ("shell-integration/wezterm.sh", "etc/profile.d/wezterm.sh"),
            (
                "shell-completion/bash",
                "share/bash-completion/completions/wezterm",
            ),
            ("shell-completion/zsh", "share/zsh/site-functions/_wezterm"),
        ] {
            copy_file(&assets.join(original), &bundle.join(relative))?;
        }
    } else {
        for name in ["shell-integration", "shell-completion"] {
            copy_tree(&assets.join(name), &resources.join(name))?;
        }
    }
    Ok(())
}

pub fn copy_windows_runtime(source: &Path, destinations: &[PathBuf]) -> Result<()> {
    let assets = source.join("assets/windows");
    for destination in destinations {
        fs::create_dir_all(destination)?;
        for (directory, name) in [
            ("conhost", "conpty.dll"),
            ("conhost", "OpenConsole.exe"),
            ("angle", "libEGL.dll"),
            ("angle", "libGLESv2.dll"),
        ] {
            copy_file(&assets.join(directory).join(name), &destination.join(name))?;
        }
        copy_tree(&assets.join("mesa"), &destination.join("mesa"))?;
    }
    Ok(())
}

pub fn package(
    ctx: &Context,
    metadata: &BuildMetadata,
    output: &Path,
    archive: bool,
) -> Result<PathBuf> {
    let _stage = ctx.runner.stage("bundle");
    crate::source::verify_source(&ctx.root, metadata)?;
    let name = format!("wez-vtabs-native-{}", state::safe_id(&metadata.id)?);
    fs::create_dir_all(output)?;
    let destination = output.join(name);
    if destination.exists() {
        let installed = verify(&destination)?;
        ensure!(
            installed.source_digest == metadata.source_digest
                && installed.upstream == metadata.upstream,
            "bundle already exists with different metadata"
        );
        if archive {
            let _stage = ctx.runner.stage("archive");
            archive_bundle(&destination)?;
        }
        return Ok(destination);
    }
    let staging = tempfile::Builder::new()
        .prefix(".bundle-")
        .tempdir_in(output)?;
    let bundle = staging.path();
    let source = ctx.cache.join("worktree");
    let runtime_copy = ctx.runner.stage("copy-runtime");
    let bindir = binary_dir(bundle);
    let resources = if cfg!(target_os = "macos") {
        let app = bundle.join("WezTerm.app");
        copy_tree(&source.join("assets/macos/WezTerm.app"), &app)?;
        for entry in fs::read_dir(&app)? {
            let entry = entry?;
            if entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "dylib")
            {
                fs::remove_file(entry.path())?;
            }
        }
        app.join("Contents/Resources")
    } else {
        bundle.join("share")
    };
    fs::create_dir_all(&bindir)?;
    let licenses = resources.join("licenses");
    fs::create_dir_all(&licenses)?;
    copy_file(
        &source.join("LICENSE.md"),
        &licenses.join("WezTerm-LICENSE.md"),
    )?;
    for entry in fs::read_dir(source.join("assets/fonts"))? {
        let entry = entry?;
        if entry.file_name().to_string_lossy().starts_with("LICENSE") && entry.path().is_file() {
            copy_file(&entry.path(), &licenses.join(entry.file_name()))?;
        }
    }
    for name in BINARIES {
        let artifact = metadata
            .artifacts
            .get(*name)
            .with_context(|| format!("missing build artifact: {name}"))?;
        let target = bindir.join(executable_name(name));
        copy_file(artifact, &target)?;
        if let Some(expected) = metadata.configuration["artifact_sha256"][*name].as_str() {
            ensure!(
                hash_file(&target)? == expected,
                "build artifact changed while packaging: {name}"
            );
        }
    }
    copy_file(&std::env::current_exe()?, &tool_path(bundle))?;
    shell_assets(&source, bundle, &resources)?;
    if cfg!(windows) {
        copy_windows_runtime(&source, std::slice::from_ref(&bindir))?;
    } else {
        ctx.runner.run(
            CommandSpec::new("tic")
                .args(["-xe", "wezterm", "-o"])
                .arg(resources.join("terminfo"))
                .arg(source.join("termwiz/data/wezterm.terminfo"))
                .cwd(&ctx.root),
        )?;
    }
    if cfg!(target_os = "linux") {
        copy_tree(&source.join("assets/icon"), &resources.join("icons"))?;
        for name in ["wezterm.desktop", "wezterm.appdata.xml"] {
            copy_file(&source.join("assets").join(name), &resources.join(name))?;
        }
    }
    copy_tree(&ctx.root.join("plugin"), &resources.join("plugin"))?;
    drop(runtime_copy);
    {
        let _stage = ctx.runner.stage("copy-source");
        crate::source::copy_source(&ctx.root, &bundle.join("source"))?;
        crate::source::verify_source(&bundle.join("source"), metadata)?;
    }
    state::write_json(&bundle.join("build.json"), metadata)?;
    let (marker_dir, relative_root, relative_tool) = if cfg!(target_os = "macos") {
        (
            &resources,
            "../../..",
            "WezTerm.app/Contents/MacOS/wez-vtabs",
        )
    } else if cfg!(windows) {
        (&bindir, ".", "wez-vtabs.exe")
    } else {
        (&bindir, "..", "bin/wez-vtabs")
    };
    state::write_json(
        &marker_dir.join("native-bundle.json"),
        &serde_json::json!({"root":relative_root,"capability":1,"updater_protocol":1,"tool":relative_tool}),
    )?;
    if cfg!(target_os = "macos") {
        let _stage = ctx.runner.stage("sign");
        ctx.runner.run(
            CommandSpec::new("codesign")
                .args(["--force", "--deep", "--sign", "-"])
                .arg(bundle.join("WezTerm.app"))
                .cwd(&ctx.root),
        )?;
        ctx.runner.run(
            CommandSpec::new("codesign")
                .args(["--verify", "--deep", "--strict"])
                .arg(bundle.join("WezTerm.app"))
                .cwd(&ctx.root),
        )?;
    }
    {
        let _stage = ctx.runner.stage("bundle-checksums");
        write_manifest(bundle)?;
        verify(bundle)?;
    }
    fs::rename(bundle, &destination)?;
    if archive {
        let _stage = ctx.runner.stage("archive");
        archive_bundle(&destination)?;
    }
    Ok(destination)
}

pub fn archive_bundle(destination: &Path) -> Result<PathBuf> {
    let metadata = verify(destination)?;
    let name = destination
        .file_name()
        .context("bundle name missing")?
        .to_string_lossy();
    let parent = destination.parent().context("bundle parent missing")?;
    let _archive_lock = state::Lock::acquire(&parent.join(format!(".{name}.archive.lock")))?;
    let zipped = cfg!(windows) || cfg!(target_os = "macos");
    let archive = parent.join(format!("{name}{}", if zipped { ".zip" } else { ".tar.gz" }));
    let sidecar = archive.with_file_name(format!(
        "{}.manifest.json",
        archive.file_name().unwrap().to_string_lossy()
    ));
    if archive.exists() && sidecar.exists() {
        let previous: serde_json::Value =
            state::read_json(&sidecar)?.context("archive manifest missing")?;
        ensure!(
            previous["id"] == metadata.id && previous["sha256"] == hash_file(&archive)?,
            "archive checksum mismatch"
        );
        return Ok(archive);
    }
    let staging = tempfile::Builder::new()
        .prefix(".archive-")
        .tempdir_in(parent)?;
    let temporary = staging.path().join("archive");
    if zipped {
        use zip::write::SimpleFileOptions;
        let mut writer = zip::ZipWriter::new(File::create(&temporary)?);
        for entry in WalkDir::new(destination)
            .follow_links(false)
            .sort_by_file_name()
        {
            let entry = entry?;
            let name = entry
                .path()
                .strip_prefix(parent)?
                .to_string_lossy()
                .replace('\\', "/");
            let permissions = {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    fs::symlink_metadata(entry.path())?.permissions().mode()
                }
                #[cfg(not(unix))]
                {
                    if entry.file_type().is_dir() {
                        0o755
                    } else {
                        0o644
                    }
                }
            };
            let options = SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated)
                .unix_permissions(permissions);
            if entry.file_type().is_dir() {
                writer.add_directory(format!("{name}/"), options)?;
            } else if entry.file_type().is_symlink() {
                writer.add_symlink(
                    name,
                    fs::read_link(entry.path())?.to_string_lossy(),
                    options,
                )?;
            } else {
                writer.start_file(name, options)?;
                std::io::copy(&mut File::open(entry.path())?, &mut writer)?;
            }
        }
        writer.finish()?.sync_all()?;
    } else {
        let encoder =
            flate2::write::GzEncoder::new(File::create(&temporary)?, flate2::Compression::fast());
        let mut writer = tar::Builder::new(encoder);
        writer.follow_symlinks(false);
        writer.append_dir_all(name.as_ref(), destination)?;
        writer.into_inner()?.finish()?.sync_all()?;
    }
    if archive.exists() {
        // An archive without its manifest is an interrupted publication. Its
        // replacement is ready before removing it, including on Windows.
        fs::remove_file(&archive)?;
    }
    fs::rename(&temporary, &archive)?;
    state::write_json(
        &sidecar,
        &serde_json::json!({
            "schema_version":1,"id":metadata.id,"target":metadata.target,
            "source_digest":metadata.source_digest,"upstream":metadata.upstream,
        "project_source":metadata.project_source,"archive":archive.file_name().unwrap().to_string_lossy(),
            "sha256":hash_file(&archive)?,"size":fs::metadata(&archive)?.len()
        }),
    )?;
    Ok(archive)
}

/// Extract only regular files, directories and safe relative symlinks. Symlinks
/// are created last so no archive entry can write through a previous link.
pub fn extract_archive(archive: &Path, destination: &Path) -> Result<PathBuf> {
    fs::create_dir_all(destination)?;
    let mut links = Vec::new();
    let mut directories = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    if archive.extension().is_some_and(|value| value == "zip") {
        let mut archive = zip::ZipArchive::new(File::open(archive)?)?;
        for index in 0..archive.len() {
            let mut file = archive.by_index(index)?;
            let relative = file.enclosed_name().context("unsafe archive path")?;
            safe_relative(&relative)?;
            ensure!(seen.insert(relative.clone()), "duplicate archive path");
            let path = destination.join(&relative);
            let mode = file.unix_mode().unwrap_or(0o644);
            if file.is_dir() {
                fs::create_dir_all(&path)?;
                directories.push((path, mode));
            } else if mode & 0o170000 == 0o120000 {
                let mut target = String::new();
                file.read_to_string(&mut target)?;
                links.push((path, PathBuf::from(target)));
            } else {
                ensure!(
                    mode & 0o170000 == 0 || mode & 0o170000 == 0o100000,
                    "unsupported archive entry"
                );
                fs::create_dir_all(path.parent().unwrap())?;
                let mut output = File::create(&path)?;
                std::io::copy(&mut file, &mut output)?;
                set_mode(&path, mode)?;
            }
        }
    } else {
        let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(File::open(archive)?));
        for file in archive.entries()? {
            let mut file = file?;
            let relative = file.path()?.into_owned();
            safe_relative(&relative)?;
            ensure!(seen.insert(relative.clone()), "duplicate archive path");
            let path = destination.join(relative);
            let kind = file.header().entry_type();
            if kind.is_dir() {
                fs::create_dir_all(&path)?;
                directories.push((path, file.header().mode()?));
            } else if kind.is_symlink() {
                links.push((
                    path,
                    file.link_name()?
                        .context("symlink target missing")?
                        .into_owned(),
                ));
            } else {
                ensure!(kind.is_file(), "unsupported archive entry type");
                fs::create_dir_all(path.parent().unwrap())?;
                let mut output = File::create(&path)?;
                std::io::copy(&mut file, &mut output)?;
                set_mode(&path, file.header().mode()?)?;
            }
        }
    }
    for (path, target) in links {
        validate_link(destination, &path, &target)?;
        ensure!(
            !path.exists(),
            "archive symlink conflicts with existing entry"
        );
        // Link parents must be real directories. No earlier symlink may
        // redirect creation of another archive entry outside this staging root.
        let mut ancestor = destination.to_path_buf();
        for part in path
            .parent()
            .unwrap()
            .strip_prefix(destination)?
            .components()
        {
            ancestor.push(part);
            if let Ok(metadata) = fs::symlink_metadata(&ancestor) {
                ensure!(
                    !metadata.file_type().is_symlink(),
                    "archive symlink parent is another symlink"
                );
            }
        }
        fs::create_dir_all(path.parent().unwrap())?;
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, path)?;
        #[cfg(windows)]
        {
            if path.parent().unwrap().join(&target).is_dir() {
                std::os::windows::fs::symlink_dir(target, path)?;
            } else {
                std::os::windows::fs::symlink_file(target, path)?;
            }
        }
    }
    for (path, mode) in directories.into_iter().rev() {
        set_mode(&path, mode)?;
    }
    let children = fs::read_dir(destination)?.collect::<std::io::Result<Vec<_>>>()?;
    ensure!(
        children.len() == 1 && children[0].path().is_dir(),
        "archive must contain one bundle directory"
    );
    let bundle = children[0].path();
    verify(&bundle)?;
    Ok(bundle)
}

pub fn set_mode(path: &Path, mode: u32) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(mode & 0o777))?;
    }
    #[cfg(not(unix))]
    {
        let _ = (path, mode);
    }
    Ok(())
}
