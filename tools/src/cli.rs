use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "wez-vtabs",
    version,
    about = "Build, validate and manage WezTerm bundles"
)]
pub struct Cli {
    #[arg(long, global = true, env = "WEZ_VTABS_ROOT")]
    pub project_root: Option<PathBuf>,
    #[arg(long, global = true, env = "WEZ_VTABS_CACHE")]
    pub cache: Option<PathBuf>,
    #[arg(long, global = true, env = "WEZ_VTABS_INSTALL")]
    pub install_root: Option<PathBuf>,
    #[arg(long, global = true, env = "WEZ_VTABS_UPSTREAM")]
    pub upstream: Option<String>,
    #[arg(long, global = true)]
    pub offline: bool,
    #[arg(long, global = true)]
    pub profile: Option<String>,
    #[arg(long, global = true, conflicts_with = "profile")]
    pub debug: bool,
    #[arg(long, global = true, value_parser = clap::value_parser!(u16).range(1..))]
    pub jobs: Option<u16>,
    #[arg(long, global = true)]
    pub json: bool,
    #[arg(long, global = true)]
    pub explain: bool,
    #[arg(long, global = true)]
    pub timings: bool,
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Resolve upstream and synchronize the checkout.
    Prepare,
    /// Run the selected upstream system dependency installer.
    Deps,
    /// Compile and validate binaries.
    Build(BuildArgs),
    /// Build an iteration bundle and launch a separate GUI.
    Dev {
        #[arg(long)]
        watch: bool,
        #[arg(long, default_value_t = 250)]
        debounce_ms: u64,
        #[arg(last = true)]
        args: Vec<String>,
    },
    /// Run the same project checks as CI.
    Check,
    /// Run a focused suite; extra pytest arguments follow --.
    Test {
        #[arg(value_enum, default_value_t = Suite::All)]
        suite: Suite,
        #[arg(last = true)]
        args: Vec<String>,
    },
    /// Generate or verify schema, Lua types and option documentation.
    Generate {
        #[arg(long)]
        check: bool,
    },
    /// Build a verified platform bundle and compressed archive.
    Package {
        #[arg(long)]
        output: Option<PathBuf>,
        #[arg(long)]
        bundle: Option<PathBuf>,
    },
    /// Install a verified immutable bundle.
    Install {
        #[arg(long)]
        bundle: Option<PathBuf>,
        #[arg(long)]
        stage_only: bool,
    },
    /// Build, install and replace the desktop application with the active version.
    Deploy {
        #[arg(long)]
        bundle: Option<PathBuf>,
        /// macOS application bundle or Linux desktop entry to replace.
        #[arg(long, conflicts_with = "no_app")]
        app: Option<PathBuf>,
        /// Linux directory for the wezterm and wezterm-gui links; default ~/.local/bin.
        #[arg(long, conflicts_with = "no_app")]
        bin: Option<PathBuf>,
        /// Install only; keep the desktop application as it is.
        #[arg(long)]
        no_app: bool,
    },
    /// Resolve updates or install a completed source/prebuilt update.
    Update {
        #[arg(long)]
        daily: bool,
        #[arg(long)]
        stage_only: bool,
        #[arg(long = "check")]
        check_only: bool,
        #[arg(long)]
        output: Option<PathBuf>,
        /// Verified prebuilt release manifest URL or file.
        #[arg(long)]
        manifest: Option<String>,
    },
    /// Launch the active version and promote a completed pending update.
    Launch {
        #[arg(last = true)]
        args: Vec<String>,
    },
    /// Diagnose prerequisites for a command.
    Doctor {
        #[arg(long = "for", default_value = "build")]
        operation: String,
    },
    /// Explain operation inputs and cache decisions without fetching or building.
    Plan {
        #[arg(default_value = "build", value_parser = ["build", "prepare", "dev", "check", "package"])]
        operation: String,
    },
    Status,
    Versions,
    /// Select an installed version, or the preceding version.
    Rollback {
        id: Option<String>,
    },
    Cache {
        #[command(subcommand)]
        command: CacheCommand,
    },
    Patch {
        #[command(subcommand)]
        command: PatchCommand,
    },
    /// Inspect a failure report; --execute reruns it in isolated state.
    Repro {
        report: PathBuf,
        #[arg(long)]
        execute: bool,
    },
    /// Create a checksum manifest for a locally assembled bundle.
    Manifest {
        #[arg(long)]
        bundle: PathBuf,
    },
    /// Verify bundle identity, contents and target.
    Verify {
        #[arg(long)]
        bundle: PathBuf,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum Suite {
    All,
    Tools,
    Rust,
    Lua,
    Gui,
    Ssh,
    Tls,
}

#[derive(Debug, Subcommand)]
pub enum CacheCommand {
    Inspect,
    /// Prune old owned run reports and distribution bundles.
    Gc {
        #[arg(long)]
        dry_run: bool,
        #[arg(long, default_value_t = 5)]
        keep: usize,
    },
}

#[derive(Debug, Subcommand)]
pub enum PatchCommand {
    Check,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct TargetOs {
    pub distro: String,
    pub version: String,
    pub image: String,
}

#[derive(Debug, Parser, Default, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BuildArgs {
    /// Build inside Ubuntu container (default version: 26.04)
    #[arg(long)]
    pub ubuntu: bool,
    /// Build inside Debian container (default version: latest)
    #[arg(long)]
    pub debian: bool,
    /// Build inside Fedora container (default version: latest)
    #[arg(long)]
    pub fedora: bool,
    /// Build inside Arch Linux container
    #[arg(long)]
    pub arch: bool,
    /// Build inside Alpine Linux container
    #[arg(long)]
    pub alpine: bool,
    /// Target distribution name (e.g. ubuntu, debian, fedora, arch, alpine)
    #[arg(long)]
    pub os: Option<String>,
    /// Target distribution version (e.g. 26.04, 24.04, 12, latest)
    #[arg(long = "os-version", alias = "distro-version")]
    pub os_version: Option<String>,
    /// Target version 26 / 26.04 (e.g. with --ubuntu)
    #[arg(long = "26", alias = "26.04")]
    pub v26: bool,
    /// Target version 24 / 24.04 (e.g. with --ubuntu)
    #[arg(long = "24", alias = "24.04")]
    pub v24: bool,
    /// Target version 22 / 22.04 (e.g. with --ubuntu)
    #[arg(long = "22", alias = "22.04")]
    pub v22: bool,
    /// Target version 12 (e.g. with --debian)
    #[arg(long = "12", alias = "12.0")]
    pub v12: bool,
    /// Target version 13 (e.g. with --debian)
    #[arg(long = "13", alias = "13.0")]
    pub v13: bool,
    /// Target version 41 (e.g. with --fedora)
    #[arg(long = "41")]
    pub v41: bool,
    /// Target version 42 (e.g. with --fedora)
    #[arg(long = "42")]
    pub v42: bool,
    /// Explicit container base image to build with (e.g. docker.io/library/ubuntu:26.04)
    #[arg(long)]
    pub image: Option<String>,
    /// Container runtime to use (docker or podman; default: auto-detect)
    #[arg(long)]
    pub container_runtime: Option<String>,
    /// Directory where built binaries should be placed (default: dist/<os>-<version>)
    #[arg(long)]
    pub output_dir: Option<PathBuf>,
    /// Skip running tests during container build
    #[arg(long)]
    pub skip_tests: bool,
    /// Rebuild the container builder image from scratch
    #[arg(long)]
    pub clean_builder: bool,
    /// Filter building only specific binary (e.g. wezterm, wez-vtabs-store)
    #[arg(long = "bin")]
    pub bin_filter: Option<String>,
}

impl BuildArgs {
    pub fn has_target_os(&self) -> bool {
        self.ubuntu
            || self.debian
            || self.fedora
            || self.arch
            || self.alpine
            || self.os.is_some()
            || self.image.is_some()
            || self.v26
            || self.v24
            || self.v22
            || self.v12
            || self.v13
            || self.v41
            || self.v42
    }

    pub fn resolve_target_os(&self) -> anyhow::Result<Option<TargetOs>> {
        if !self.has_target_os() {
            return Ok(None);
        }

        if let Some(ref img) = self.image {
            let (distro, version) = parse_image_distro_version(img);
            return Ok(Some(TargetOs {
                distro,
                version,
                image: img.clone(),
            }));
        }

        let distro = if self.ubuntu || self.v26 || self.v24 || self.v22 {
            "ubuntu"
        } else if self.debian || self.v12 || self.v13 {
            "debian"
        } else if self.fedora || self.v41 || self.v42 {
            "fedora"
        } else if self.arch {
            "arch"
        } else if self.alpine {
            "alpine"
        } else if let Some(ref os) = self.os {
            os.as_str()
        } else {
            "ubuntu"
        }
        .to_lowercase();

        let version = match distro.as_str() {
            "ubuntu" => {
                if self.v26 {
                    "26.04"
                } else if self.v24 {
                    "24.04"
                } else if self.v22 {
                    "22.04"
                } else {
                    match self.os_version.as_deref() {
                        Some("26") => "26.04",
                        Some("24") => "24.04",
                        Some("22") => "22.04",
                        Some(v) => v,
                        None => "26.04",
                    }
                }
            }
            "debian" => {
                if self.v12 {
                    "12"
                } else if self.v13 {
                    "13"
                } else {
                    self.os_version.as_deref().unwrap_or("latest")
                }
            }
            "fedora" => {
                if self.v41 {
                    "41"
                } else if self.v42 {
                    "42"
                } else {
                    self.os_version.as_deref().unwrap_or("latest")
                }
            }
            "arch" | "archlinux" => self.os_version.as_deref().unwrap_or("base"),
            _ => self.os_version.as_deref().unwrap_or("latest"),
        }
        .to_string();

        let image = match distro.as_str() {
            "arch" | "archlinux" => "docker.io/library/archlinux:base".to_string(),
            _ => format!("docker.io/library/{}:{}", distro, version),
        };

        Ok(Some(TargetOs {
            distro,
            version,
            image,
        }))
    }
}

fn parse_image_distro_version(image: &str) -> (String, String) {
    let tag_part = image.rsplit('/').next().unwrap_or(image);
    if let Some((name, tag)) = tag_part.split_once(':') {
        (name.to_string(), tag.to_string())
    } else {
        (tag_part.to_string(), "latest".to_string())
    }
}
