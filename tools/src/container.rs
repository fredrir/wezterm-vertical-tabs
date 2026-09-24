//! Builds in containers with their own cache: x86_64 Linux bundles, and macOS binaries
//! cross-compiled for a Mac to package and sign.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, bail, ensure};
use serde_json::{Value, json};

use crate::process::CommandSpec;
use crate::state::{self, BuildMetadata, Context};
use crate::targets::Platform;

const PLATFORM: &str = "linux/amd64";
pub const MACOS_TRIPLE: &str = "aarch64-apple-darwin";
const MACOS_BASE: &str = "docker.io/library/archlinux:base";

/// clang, lld and Rust for aarch64-apple-darwin; the SDK is mounted at /sdk.
const MACOS_DOCKERFILE: &str = r#"ARG BASE_IMAGE
FROM ${BASE_IMAGE}

ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:$PATH

RUN printf 'Server = %s/$repo/os/$arch\n' \
        https://geo.mirror.pkgbuild.com \
        https://mirror.rackspace.com/archlinux \
        https://mirrors.kernel.org/archlinux \
        https://fastly.mirror.pkgbuild.com \
        > /etc/pacman.d/mirrorlist \
    && pacman -Syu --noconfirm --needed --disable-download-timeout \
        base-devel clang lld llvm cmake git perl pkgconf python rustup file \
    && pacman -Scc --noconfirm

RUN mkdir -p /usr/local/cargo /usr/local/rustup \
    && rustup toolchain install stable --profile minimal --target aarch64-apple-darwin \
    && rustup default stable \
    && chmod -R a+rwX /usr/local/cargo /usr/local/rustup

RUN git config --system --add safe.directory '*'

# One driver for cc-rs, build-script link probes and rustc's final link.
RUN for driver in clang clang++; do \
        printf '#!/bin/sh\nexec %s --target=arm64-apple-macos"$MACOSX_DEPLOYMENT_TARGET" -isysroot "$SDKROOT" -fuse-ld=lld -Wno-unused-command-line-argument "$@"\n' "$driver" \
            > "/usr/local/bin/aarch64-apple-darwin-$driver"; \
        chmod 755 "/usr/local/bin/aarch64-apple-darwin-$driver"; \
    done

ENV SDKROOT=/sdk/MacOSX.sdk \
    MACOSX_DEPLOYMENT_TARGET=11.0 \
    CC_aarch64_apple_darwin=aarch64-apple-darwin-clang \
    CXX_aarch64_apple_darwin=aarch64-apple-darwin-clang++ \
    AR_aarch64_apple_darwin=llvm-ar \
    RANLIB_aarch64_apple_darwin=llvm-ranlib \
    CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER=aarch64-apple-darwin-clang
"#;

#[derive(Debug, Default)]
pub struct Options {
    pub image: Option<String>,
    pub runtime: Option<String>,
    pub clean: bool,
}

/// `start` launches Docker Desktop when x86_64 builds on a Mac need it.
pub fn detect_runtime(explicit: Option<&str>, start: bool) -> Result<String> {
    if let Some(runtime) = explicit {
        ensure!(which(runtime), "container runtime not found: {runtime}");
        return Ok(runtime.to_string());
    }
    for var in ["VTABS_CONTAINER_RUNTIME", "VTABS_TEST_CONTAINER_RUNTIME"] {
        if let Some(runtime) = std::env::var(var).ok().filter(|r| which(r)) {
            return Ok(runtime);
        }
    }
    if cfg!(target_os = "macos") && emulated() {
        return docker_desktop(start);
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

/// Docker Desktop runs x86_64 containers through Rosetta; podman's default libkrun VM cannot.
fn docker_desktop(start: bool) -> Result<String> {
    ensure!(
        which("docker"),
        "x86_64 builds on Apple silicon need Docker Desktop"
    );
    if start && !is_runtime_working("docker") {
        eprintln!("+ open -g -a Docker");
        std::process::Command::new("open")
            .args(["-g", "-a", "Docker"])
            .status()
            .context("start: Docker Desktop")?;
        let started = std::time::Instant::now();
        while !is_runtime_working("docker") {
            ensure!(
                started.elapsed() < std::time::Duration::from_secs(120),
                "Docker Desktop did not start"
            );
            std::thread::sleep(std::time::Duration::from_secs(2));
        }
    }
    Ok("docker".into())
}

fn which(cmd: &str) -> bool {
    if Path::new(cmd).is_absolute() {
        return Path::new(cmd).is_file();
    }
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(cmd).is_file()))
}

fn is_runtime_working(cmd: &str) -> bool {
    std::process::Command::new(cmd)
        .arg("info")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn image_exists(runtime: &str, tag: &str) -> bool {
    std::process::Command::new(runtime)
        .args(["image", "inspect", tag])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn is_podman(runtime: &str) -> bool {
    Path::new(runtime)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(runtime)
        .contains("podman")
}

/// Apple silicon and other non-x86_64 hosts run the builder emulated.
fn emulated() -> bool {
    std::env::consts::ARCH != "x86_64"
}

/// Copied from a Mac by `remote::ensure_sdk`, independent of the build cache.
pub fn sdk_dir() -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::home().join(".cache"))
        .join("wez-vtabs/sdk")
}

pub fn builder_image_tag(platform: &Platform) -> String {
    match platform {
        Platform::Macos => "localhost/wez-vtabs-builder:macos-cross".into(),
        Platform::Linux(distro) => format!(
            "localhost/wez-vtabs-builder:{}-{}-x86_64",
            distro.name, distro.version
        ),
    }
}

fn base_image<'a>(platform: &'a Platform, options: &'a Options) -> &'a str {
    options.image.as_deref().unwrap_or(match platform {
        Platform::Macos => MACOS_BASE,
        Platform::Linux(distro) => &distro.image,
    })
}

pub fn cache_dir(ctx: &Context, platform: &Platform) -> PathBuf {
    ctx.cache.join("platforms").join(platform.to_string())
}

pub fn builder_dockerfile(platform: &Platform) -> String {
    let Platform::Linux(distro) = platform else {
        return MACOS_DOCKERFILE.into();
    };
    let pkg_install = match distro.name.as_str() {
        "ubuntu" | "debian" | "pop" => {
            r#"RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl git build-essential cmake pkg-config python3 ncurses-bin \
    libegl1-mesa-dev libssl-dev libfontconfig1-dev libwayland-dev \
    libx11-xcb-dev libxcb-ewmh-dev libxcb-icccm4-dev libxcb-image0-dev \
    libxcb-keysyms1-dev libxcb-randr0-dev libxcb-render0-dev libxcb-xkb-dev \
    libxkbcommon-dev libxkbcommon-x11-dev libxcb-util-dev \
    && rm -rf /var/lib/apt/lists/*"#
        }
        "fedora" | "centos" | "rhel" | "rocky" | "alma" => {
            r#"RUN dnf install -y make gcc gcc-c++ cmake git pkgconf python3 ncurses \
    fontconfig-devel openssl-devel libxcb-devel libxkbcommon-devel \
    libxkbcommon-x11-devel wayland-devel mesa-libEGL-devel \
    xcb-util-devel xcb-util-keysyms-devel xcb-util-image-devel xcb-util-wm-devel \
    curl ca-certificates"#
        }
        "alpine" => {
            r#"RUN apk add --no-cache build-base cmake git pkgconf python3 ncurses \
    fontconfig-dev libx11-dev libxkbcommon-dev openssl-dev \
    wayland-dev xcb-util-dev xcb-util-image-dev xcb-util-keysyms-dev \
    xcb-util-wm-dev zlib-dev zstd-dev curl ca-certificates bash"#
        }
        "arch" => {
            r#"RUN pacman -Syu --noconfirm base-devel cmake git pkgconf python3 ncurses \
    fontconfig libx11 libxkbcommon-x11 wayland xcb-util \
    xcb-util-image xcb-util-keysyms xcb-util-wm curl rust \
    && pacman -Scc --noconfirm"#
        }
        _ => {
            r#"RUN if command -v apt-get >/dev/null 2>&1; then \
        apt-get update && apt-get install -y --no-install-recommends \
            ca-certificates curl git build-essential cmake pkg-config python3 ncurses-bin \
            libegl1-mesa-dev libssl-dev libfontconfig1-dev libwayland-dev \
            libx11-xcb-dev libxcb-ewmh-dev libxcb-icccm4-dev libxcb-image0-dev \
            libxcb-keysyms1-dev libxcb-randr0-dev libxcb-render0-dev libxcb-xkb-dev \
            libxkbcommon-dev libxkbcommon-x11-dev libxcb-util-dev \
            && rm -rf /var/lib/apt/lists/*; \
    elif command -v dnf >/dev/null 2>&1; then \
        dnf install -y make gcc gcc-c++ cmake git pkgconf python3 ncurses \
            fontconfig-devel openssl-devel libxcb-devel libxkbcommon-devel \
            libxkbcommon-x11-devel wayland-devel mesa-libEGL-devel \
            xcb-util-devel xcb-util-keysyms-devel xcb-util-image-devel xcb-util-wm-devel \
            curl ca-certificates; \
    elif command -v apk >/dev/null 2>&1; then \
        apk add --no-cache build-base cmake git pkgconf python3 ncurses \
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

fn ensure_builder_image(
    ctx: &Context,
    runtime: &str,
    platform: &Platform,
    base: &str,
    clean: bool,
) -> Result<String> {
    let tag = builder_image_tag(platform);
    if !clean && image_exists(runtime, &tag) {
        return Ok(tag);
    }
    let _stage = ctx.runner.stage(&format!("builder-image:{platform}"));
    let temp_dir = tempfile::tempdir().context("create tempdir for dockerfile")?;
    let dockerfile = temp_dir.path().join("Dockerfile");
    fs::write(&dockerfile, builder_dockerfile(platform))?;
    let mut build = CommandSpec::new(runtime).arg("build");
    if emulated() {
        build = build.args(["--platform", PLATFORM]);
    }
    ctx.runner
        .run(
            build
                .args([
                    "--build-arg",
                    &format!("BASE_IMAGE={base}"),
                    "-t",
                    &tag,
                    "-f",
                ])
                .arg(&dockerfile)
                .arg(temp_dir.path()),
        )
        .with_context(|| format!("builder image for {platform} ({base})"))?;
    Ok(tag)
}

fn pinned_upstream(ctx: &Context, cache: &Path) -> Result<Option<String>> {
    if ctx.upstream.is_some() {
        return Ok(ctx.upstream.clone());
    }
    let previous: Option<BuildMetadata> = state::read_json(&cache.join("build.json"))?;
    Ok(previous.map(|previous| previous.upstream))
}

fn tool_arguments(
    ctx: &Context,
    platform: &Platform,
    cache: &Path,
    output: &Path,
) -> Result<Vec<String>> {
    let mut arguments = vec![
        "cargo".into(),
        "xtask".into(),
        "--project-root".into(),
        ctx.root.display().to_string(),
        "--cache".into(),
        cache.display().to_string(),
        "--install-root".into(),
        cache.join("install").display().to_string(),
        "--profile".into(),
        ctx.profile.clone(),
    ];
    if let Some(upstream) = pinned_upstream(ctx, cache)? {
        arguments.extend(["--upstream".into(), upstream]);
    }
    if ctx.offline {
        arguments.push("--offline".into());
    }
    if let Some(jobs) = ctx.jobs {
        arguments.extend(["--jobs".into(), jobs.to_string()]);
    }
    let command: &[&str] = match platform {
        Platform::Macos => &["prebuild", "--triple", MACOS_TRIPLE],
        Platform::Linux(_) => &["package", "--role", ctx.role.name(), "--no-archive"],
    };
    arguments.extend(command.iter().map(|value| value.to_string()));
    arguments.extend(["--output".into(), output.display().to_string()]);
    Ok(arguments)
}

pub fn plan(ctx: &Context, platform: &Platform, options: &Options, output: &Path) -> Result<Value> {
    let cache = cache_dir(ctx, platform);
    Ok(json!({
        "platform": platform.to_string(),
        "base_image": base_image(platform, options),
        "builder_image": builder_image_tag(platform),
        "runtime": detect_runtime(options.runtime.as_deref(), false).unwrap_or_else(|_| "docker/podman".into()),
        "emulated": emulated(),
        "role": ctx.role,
        "cache": cache,
        "output": output,
        "command": tool_arguments(ctx, platform, &cache, output)?,
    }))
}

/// Linux: a verified bundle directory. macOS: a prebuilt directory for `deploy --prebuilt`.
pub fn build(
    ctx: &Context,
    platform: &Platform,
    options: &Options,
    output: &Path,
) -> Result<PathBuf> {
    let sdk = sdk_dir();
    if *platform == Platform::Macos {
        ensure!(
            sdk.join("MacOSX.sdk/SDKSettings.json").is_file(),
            "macOS SDK missing: {}; deploy a macOS target from a Mac once",
            sdk.display()
        );
    }
    let runtime = detect_runtime(options.runtime.as_deref(), true)?;
    let base = base_image(platform, options);
    let tag = ensure_builder_image(ctx, &runtime, platform, base, options.clean)?;
    let cache = cache_dir(ctx, platform);
    fs::create_dir_all(&cache)?;
    fs::create_dir_all(output)?;
    let cache = cache.canonicalize()?;
    let output = output.canonicalize()?;
    let _stage = ctx.runner.stage(&format!("container-build:{platform}"));

    let mut run = CommandSpec::new(&runtime).args(["run", "--rm"]);
    if emulated() {
        run = run.args(["--platform", PLATFORM]);
    }
    #[cfg(unix)]
    {
        let (uid, gid) = unsafe { (libc::getuid(), libc::getgid()) };
        if is_podman(&runtime) && uid != 0 {
            run = run.arg("--userns=keep-id");
        }
        run = run.args(["--user", &format!("{uid}:{gid}")]);
    }
    for path in [&ctx.root, &cache, &output] {
        run = run.args(["-v", &format!("{}:{}", path.display(), path.display())]);
    }
    if *platform == Platform::Macos {
        run = run.args(["-v", &format!("{}:/sdk:ro", sdk.display())]);
    }
    // Synced remote sources record their project revision beside the source root.
    if let Some(record) = ctx
        .root
        .parent()
        .map(|parent| parent.join("build.json"))
        .filter(|path| path.is_file())
    {
        run = run.args([
            "-v",
            &format!("{}:{}:ro", record.display(), record.display()),
        ]);
    }
    let cargo_home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::home().join(".cargo"));
    for name in ["registry", "git"] {
        if cargo_home.join(name).is_dir() {
            run = run.args([
                "-v",
                &format!(
                    "{}:/usr/local/cargo/{name}",
                    cargo_home.join(name).display()
                ),
            ]);
        }
    }
    run = run.args([
        "-e",
        &format!("HOME={}", cache.display()),
        "-e",
        "GIT_CONFIG_COUNT=1",
        "-e",
        "GIT_CONFIG_KEY_0=safe.directory",
        "-e",
        "GIT_CONFIG_VALUE_0=*",
        "-e",
        &format!("CARGO_TARGET_DIR={}", cache.join("xtask-target").display()),
        "-w",
        &ctx.root.display().to_string(),
        &tag,
    ]);
    let output_json = ctx
        .runner
        .capture(run.args(tool_arguments(ctx, platform, &cache, &output)?))
        .with_context(|| format!("container build failed for {platform} ({base})"))?;
    let result: Value =
        serde_json::from_str(&output_json).context("container build printed no result")?;
    let key = match platform {
        Platform::Macos => "prebuilt",
        Platform::Linux(_) => "bundle",
    };
    let bundle = PathBuf::from(
        result[key]
            .as_str()
            .context("container build result missing")?,
    );
    ensure!(
        bundle.is_dir(),
        "container bundle missing: {}",
        bundle.display()
    );
    Ok(bundle)
}
