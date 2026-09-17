use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context as _, Result, bail, ensure};
use serde_json::{Value, json};

use crate::bundle::hash_file;
use crate::cli::{BuildArgs, TargetOs};
use crate::process::CommandSpec;
use crate::source;
use crate::state::{self, Context};

pub const CONTAINER_BINARIES: &[&str] = &[
    "wezterm-gui",
    "wezterm",
    "wezterm-mux-server",
    "strip-ansi-escapes",
    "wez-vtabs-store",
    "wez-vtabs",
];

pub fn plan(ctx: &Context, args: &BuildArgs, target: &TargetOs) -> Result<Value> {
    let output_dir = resolve_output_dir(ctx, args, target);
    let builder_tag = builder_image_tag(target);
    let runtime = detect_runtime(args.container_runtime.as_deref())
        .unwrap_or_else(|_| "docker/podman".into());
    let binaries = if let Some(ref filter) = args.bin_filter {
        vec![filter.as_str()]
    } else {
        CONTAINER_BINARIES.to_vec()
    };
    Ok(json!({
        "target_os": target.distro,
        "version": target.version,
        "base_image": target.image,
        "builder_image": builder_tag,
        "runtime": runtime,
        "output_dir": output_dir.display().to_string(),
        "binaries": binaries,
        "skip_tests": args.skip_tests,
        "clean_builder": args.clean_builder,
    }))
}

pub fn detect_runtime(explicit: Option<&str>) -> Result<String> {
    if let Some(runtime) = explicit {
        ensure!(
            which(runtime),
            "specified container runtime '{runtime}' was not found in PATH"
        );
        return Ok(runtime.to_string());
    }

    for var in ["VTABS_CONTAINER_RUNTIME", "VTABS_TEST_CONTAINER_RUNTIME"] {
        if let Some(runtime) = std::env::var(var).ok().filter(|r| which(r)) {
            return Ok(runtime);
        }
    }

    let candidates = ["podman", "docker"];
    if let Some(&candidate) = candidates
        .iter()
        .find(|&&c| which(c) && is_runtime_working(c))
    {
        return Ok(candidate.to_string());
    }
    if let Some(&candidate) = candidates.iter().find(|&&c| which(c)) {
        return Ok(candidate.to_string());
    }

    bail!("container build requires Podman or Docker in PATH");
}

fn which(cmd: &str) -> bool {
    if Path::new(cmd).is_absolute() {
        return Path::new(cmd).is_file();
    }
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            let full = dir.join(cmd);
            if full.is_file() {
                return true;
            }
        }
    }
    false
}

fn is_runtime_working(cmd: &str) -> bool {
    std::process::Command::new(cmd)
        .arg("info")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn image_exists(runtime: &str, tag: &str) -> bool {
    std::process::Command::new(runtime)
        .args(["image", "inspect", tag])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn is_podman(runtime: &str) -> bool {
    let file = Path::new(runtime)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(runtime);
    file.contains("podman")
}

pub fn builder_image_tag(target: &TargetOs) -> String {
    format!(
        "localhost/wez-vtabs-builder:{}-{}",
        target.distro, target.version
    )
}

pub fn builder_dockerfile(target: &TargetOs) -> String {
    let distro = target.distro.as_str();
    let pkg_install = match distro {
        "ubuntu" | "debian" | "pop" => {
            r#"RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl git build-essential cmake pkg-config python3 \
    libegl1-mesa-dev libssl-dev libfontconfig1-dev libwayland-dev \
    libx11-xcb-dev libxcb-ewmh-dev libxcb-icccm4-dev libxcb-image0-dev \
    libxcb-keysyms1-dev libxcb-randr0-dev libxcb-render0-dev libxcb-xkb-dev \
    libxkbcommon-dev libxkbcommon-x11-dev libxcb-util-dev \
    && rm -rf /var/lib/apt/lists/*"#
        }
        "fedora" | "centos" | "rhel" | "rocky" | "alma" => {
            r#"RUN dnf install -y make gcc gcc-c++ cmake git pkgconf python3 \
    fontconfig-devel openssl-devel libxcb-devel libxkbcommon-devel \
    libxkbcommon-x11-devel wayland-devel mesa-libEGL-devel \
    xcb-util-devel xcb-util-keysyms-devel xcb-util-image-devel xcb-util-wm-devel \
    curl ca-certificates"#
        }
        "alpine" => {
            r#"RUN apk add --no-cache build-base cmake git pkgconf python3 \
    fontconfig-dev libx11-dev libxkbcommon-dev openssl-dev \
    wayland-dev xcb-util-dev xcb-util-image-dev xcb-util-keysyms-dev \
    xcb-util-wm-dev zlib-dev zstd-dev curl ca-certificates bash"#
        }
        "arch" | "archlinux" => {
            r#"RUN pacman -Syu --noconfirm base-devel cmake git pkgconf python3 \
    fontconfig libx11 libxkbcommon-x11 wayland xcb-util \
    xcb-util-image xcb-util-keysyms xcb-util-wm curl rust cargo \
    && pacman -Scc --noconfirm"#
        }
        _ => {
            r#"RUN if command -v apt-get >/dev/null 2>&1; then \
        apt-get update && apt-get install -y --no-install-recommends \
            ca-certificates curl git build-essential cmake pkg-config python3 \
            libegl1-mesa-dev libssl-dev libfontconfig1-dev libwayland-dev \
            libx11-xcb-dev libxcb-ewmh-dev libxcb-icccm4-dev libxcb-image0-dev \
            libxcb-keysyms1-dev libxcb-randr0-dev libxcb-render0-dev libxcb-xkb-dev \
            libxkbcommon-dev libxkbcommon-x11-dev libxcb-util-dev \
            && rm -rf /var/lib/apt/lists/*; \
    elif command -v dnf >/dev/null 2>&1; then \
        dnf install -y make gcc gcc-c++ cmake git pkgconf python3 \
            fontconfig-devel openssl-devel libxcb-devel libxkbcommon-devel \
            libxkbcommon-x11-devel wayland-devel mesa-libEGL-devel \
            xcb-util-devel xcb-util-keysyms-devel xcb-util-image-devel xcb-util-wm-devel \
            curl ca-certificates; \
    elif command -v apk >/dev/null 2>&1; then \
        apk add --no-cache build-base cmake git pkgconf python3 \
            fontconfig-dev libx11-dev libxkbcommon-dev openssl-dev \
            wayland-dev xcb-util-dev xcb-util-image-dev xcb-util-keysyms-dev \
            xcb-util-wm-dev zlib-dev zstd-dev curl ca-certificates bash; \
    fi"#
        }
    };

    format!(
        r#"ARG BASE_IMAGE
FROM ${{BASE_IMAGE}}

ENV DEBIAN_FRONTEND=noninteractive
ENV RUSTUP_HOME=/usr/local/rustup
ENV CARGO_HOME=/usr/local/cargo
ENV PATH=/usr/local/cargo/bin:$PATH

{pkg_install}

RUN git config --system --add safe.directory '*' || true

# Ensure Rust 1.85+ is installed
RUN if ! (command -v rustc >/dev/null && command -v cargo >/dev/null && [ "$(rustc --version | awk '{{print $2}}' | cut -d'.' -f2)" -ge 85 ] 2>/dev/null); then \
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable --profile minimal \
        && chmod -R a+rwX /usr/local/cargo /usr/local/rustup 2>/dev/null || true; \
    fi
"#
    )
}

pub fn ensure_builder_image(
    ctx: &Context,
    runtime: &str,
    target: &TargetOs,
    clean: bool,
) -> Result<String> {
    let tag = builder_image_tag(target);

    if !clean && image_exists(runtime, &tag) {
        return Ok(tag);
    }

    let _stage = ctx
        .runner
        .stage(&format!("builder-image:{}", target.distro));

    let dockerfile_content = builder_dockerfile(target);
    let temp_dir = tempfile::tempdir().context("create tempdir for dockerfile")?;
    let dockerfile_path = temp_dir.path().join("Dockerfile");
    fs::write(&dockerfile_path, &dockerfile_content)?;

    let build_spec = CommandSpec::new(runtime).args([
        "build",
        "--build-arg",
        &format!("BASE_IMAGE={}", target.image),
        "-t",
        &tag,
        "-f",
        &dockerfile_path.display().to_string(),
        &temp_dir.path().display().to_string(),
    ]);

    ctx.runner.run(build_spec).with_context(|| {
        format!(
            "failed to build container builder image for {} ({})",
            target.distro, target.image
        )
    })?;

    Ok(tag)
}

pub fn resolve_output_dir(ctx: &Context, args: &BuildArgs, target: &TargetOs) -> PathBuf {
    args.output_dir.clone().unwrap_or_else(|| {
        ctx.root
            .join("dist")
            .join(format!("{}-{}", target.distro, target.version))
    })
}

pub fn build(ctx: &Context, args: &BuildArgs, target: &TargetOs) -> Result<Value> {
    let start_time = Instant::now();

    let resolved = source::resolve(ctx)?;
    let worktree = source::prepare(ctx, &resolved)?;

    let runtime = detect_runtime(args.container_runtime.as_deref())?;
    let builder_tag = ensure_builder_image(ctx, &runtime, target, args.clean_builder)?;

    let container_cache = ctx
        .cache
        .join("containers")
        .join(format!("{}-{}", target.distro, target.version));
    let target_dir = container_cache.join("target");
    fs::create_dir_all(&target_dir)?;

    let profile = if ctx.profile == "debug" {
        "dev"
    } else {
        &ctx.profile
    };
    let profile_dir = if profile == "dev" { "debug" } else { profile };

    #[cfg(unix)]
    let (uid, gid) = unsafe { (libc::getuid(), libc::getgid()) };
    #[cfg(not(unix))]
    let (uid, gid) = (1000, 1000);

    let host_cargo_home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cargo")));

    let cargo_flags = format!(
        "{}{}",
        if ctx.offline { " --offline" } else { "" },
        ctx.jobs.map(|j| format!(" -j {j}")).unwrap_or_default(),
    );

    let (vtabs_cmd, wezterm_cmd) = match args.bin_filter.as_deref() {
        Some("wez-vtabs-store") => (
            format!(
                "cargo build --profile {profile} --locked{cargo_flags} --manifest-path {root}/Cargo.toml -p vtabs-store --features sqlite",
                root = ctx.root.display(),
            ),
            "true".to_string(),
        ),
        Some("wez-vtabs") => (
            format!(
                "cargo build --profile {profile} --locked{cargo_flags} --manifest-path {root}/Cargo.toml -p tools --bin wez-vtabs",
                root = ctx.root.display(),
            ),
            "true".to_string(),
        ),
        Some(filter) => (
            "true".to_string(),
            format!(
                "cargo build --profile {profile}{cargo_flags} --manifest-path {worktree}/Cargo.toml -p {filter}",
                worktree = worktree.display(),
            ),
        ),
        None => (
            format!(
                "cargo build --profile {profile} --locked{cargo_flags} --manifest-path {root}/Cargo.toml -p vtabs-store --features sqlite && \
                 cargo build --profile {profile} --locked{cargo_flags} --manifest-path {root}/Cargo.toml -p tools --bin wez-vtabs",
                root = ctx.root.display(),
            ),
            format!(
                "cargo build --profile {profile}{cargo_flags} --manifest-path {worktree}/Cargo.toml -p wezterm-gui -p wezterm -p wezterm-mux-server -p strip-ansi-escapes",
                worktree = worktree.display(),
            ),
        ),
    };
    let test_cmd = if args.skip_tests {
        "true".to_string()
    } else if let Some(ref filter) = args.bin_filter {
        if filter == "wezterm-gui" {
            format!(
                "cargo test --locked{cargo_flags} --manifest-path {worktree}/Cargo.toml -p wezterm-gui vtabs",
                worktree = worktree.display(),
            )
        } else if filter == "wez-vtabs-store" {
            format!(
                "cargo test --locked{cargo_flags} --manifest-path {root}/Cargo.toml -p vtabs-store --features sqlite",
                root = ctx.root.display(),
            )
        } else {
            "true".to_string()
        }
    } else {
        format!(
            "cargo test --locked{cargo_flags} --manifest-path {worktree}/Cargo.toml -p wezterm-gui vtabs && \
             cargo test --locked{cargo_flags} --manifest-path {worktree}/Cargo.toml -p wezterm-client --lib vtabs && \
             cargo test --locked{cargo_flags} --manifest-path {worktree}/Cargo.toml -p wezterm-input-types --lib vtabs",
            worktree = worktree.display(),
        )
    };

    let build_script = format!("set -e\n{vtabs_cmd}\n{wezterm_cmd}\n{test_cmd}\n");

    let _stage = ctx
        .runner
        .stage(&format!("container-build:{}", target.distro));

    let mut run_spec = CommandSpec::new(&runtime);
    run_spec = run_spec.arg("run").arg("--rm");

    // Map user
    #[cfg(unix)]
    {
        if is_podman(&runtime) && uid != 0 {
            run_spec = run_spec.args(["--userns=keep-id", "--user", &format!("{}:{}", uid, gid)]);
        } else {
            run_spec = run_spec.args(["--user", &format!("{}:{}", uid, gid)]);
        }
    }
    #[cfg(not(unix))]
    {
        run_spec = run_spec.args(["--user", &format!("{}:{}", uid, gid)]);
    }

    // Volume mounts
    run_spec = run_spec.args([
        "-v",
        &format!("{}:{}", ctx.root.display(), ctx.root.display()),
        "-v",
        &format!("{}:{}", ctx.cache.display(), ctx.cache.display()),
    ]);

    if let Some(ref cargo_home) = host_cargo_home {
        let registry = cargo_home.join("registry");
        if registry.is_dir() {
            run_spec = run_spec.args([
                "-v",
                &format!("{}:/usr/local/cargo/registry", registry.display()),
            ]);
        }
        let git_dir = cargo_home.join("git");
        if git_dir.is_dir() {
            run_spec =
                run_spec.args(["-v", &format!("{}:/usr/local/cargo/git", git_dir.display())]);
        }
    }

    if let Some(home) = std::env::var_os("HOME") {
        run_spec = run_spec.args(["-e", &format!("HOME={}", home.to_string_lossy())]);
    }

    run_spec = run_spec.args([
        "-e",
        "GIT_CONFIG_COUNT=1",
        "-e",
        "GIT_CONFIG_KEY_0=safe.directory",
        "-e",
        "GIT_CONFIG_VALUE_0=*",
        "-e",
        &format!("CARGO_TARGET_DIR={}", target_dir.display()),
        "-w",
        &worktree.display().to_string(),
        &builder_tag,
        "sh",
        "-c",
        &build_script,
    ]);

    ctx.runner.run(run_spec).with_context(|| {
        format!(
            "container build failed for target {} ({})",
            target.distro, target.image
        )
    })?;

    // Collect binaries
    let output_dir = resolve_output_dir(ctx, args, target);
    fs::create_dir_all(&output_dir)?;

    let binaries_dir = target_dir.join(profile_dir);
    let mut artifacts = serde_json::Map::new();
    let mut hashes = serde_json::Map::new();

    let target_binaries = if let Some(ref filter) = args.bin_filter {
        vec![filter.as_str()]
    } else {
        CONTAINER_BINARIES.to_vec()
    };

    for &binary_name in &target_binaries {
        let src = binaries_dir.join(binary_name);
        ensure!(
            src.is_file(),
            "expected binary '{binary_name}' was not produced at {}",
            src.display()
        );
        let dest = output_dir.join(binary_name);
        fs::copy(&src, &dest)
            .with_context(|| format!("copy {} to {}", src.display(), dest.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&dest)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&dest, perms)?;
        }
        let digest = hash_file(&dest)?;
        artifacts.insert(
            binary_name.to_string(),
            Value::String(dest.display().to_string()),
        );
        hashes.insert(binary_name.to_string(), Value::String(digest));
    }

    if target_binaries.contains(&"wez-vtabs") {
        let launcher_dest = output_dir.join("wez-vtabs-launcher");
        if !launcher_dest.exists() {
            #[cfg(unix)]
            {
                let _ = std::os::unix::fs::symlink("wez-vtabs", &launcher_dest);
            }
            #[cfg(not(unix))]
            {
                let _ = fs::copy(output_dir.join("wez-vtabs"), &launcher_dest);
            }
        }
        let desktop_dest = output_dir.join("wez-vtabs.desktop");
        if !desktop_dest.exists() {
            let desktop_content = "[Desktop Entry]\n\
                Name=WezTerm (Vertical Tabs)\n\
                Comment=WezTerm terminal emulator with vertical tabs\n\
                Type=Application\n\
                Categories=System;TerminalEmulator;\n\
                StartupWMClass=org.wezfurlong.wezterm\n\
                Exec=wez-vtabs launch\n\
                Icon=org.wezfurlong.wezterm\n\
                Terminal=false\n";
            let _ = fs::write(&desktop_dest, desktop_content);
        }
    }

    let manifest = json!({
        "target_os": target.distro,
        "version": target.version,
        "image": target.image,
        "builder_image": builder_tag,
        "profile": profile,
        "upstream": resolved.revision,
        "created_at": state::now(),
        "hashes": hashes,
        "artifacts": artifacts,
    });
    state::write_json(&output_dir.join("checksums.json"), &manifest)?;

    let archive_path = ctx.root.join("dist").join(format!(
        "wez-vtabs-{}-{}.tar.gz",
        target.distro, target.version
    ));
    archive_directory(&output_dir, &archive_path)
        .with_context(|| format!("create archive {}", archive_path.display()))?;

    let duration = start_time.elapsed().as_secs_f64();
    let result = json!({
        "status": "completed",
        "target_os": target.distro,
        "version": target.version,
        "image": target.image,
        "builder_image": builder_tag,
        "runtime": runtime,
        "profile": profile,
        "output_dir": output_dir.display().to_string(),
        "archive": archive_path.display().to_string(),
        "artifacts": artifacts,
        "hashes": hashes,
        "duration_seconds": duration,
    });

    Ok(result)
}

fn archive_directory(dir: &Path, archive: &Path) -> Result<()> {
    if let Some(parent) = archive.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = fs::File::create(archive)?;
    let enc = flate2::write::GzEncoder::new(file, flate2::Compression::default());
    let mut tar = tar::Builder::new(enc);
    tar.append_dir_all(".", dir)?;
    tar.finish()?;
    Ok(())
}
