use clap::{Parser, Subcommand, ValueEnum};
use clap_complete::engine::ArgValueCandidates;
use std::path::PathBuf;

use crate::state::Role;

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
    /// Compile and validate; --to builds a bundle for another machine.
    Build(BuildArgs),
    /// Build an iteration bundle and launch a separate GUI.
    Dev {
        /// Watch source files and hot-reload changes (default: true)
        #[arg(long, num_args = 0..=1, default_value = "true", default_missing_value = "true")]
        watch: bool,
        /// Disable watch mode and launch the GUI once
        #[arg(long)]
        no_watch: bool,
        #[arg(long, default_value_t = 150)]
        debounce_ms: u64,
        #[arg(last = true)]
        args: Vec<String>,
    },
    /// Run the same project checks as CI.
    Check,
    /// Formatting, Clippy, Ruff and generated files; --fix rewrites them.
    Lint {
        #[arg(long)]
        fix: bool,
        #[command(flatten)]
        on: On,
    },
    /// Run a focused suite; extra pytest arguments follow --.
    Test {
        #[arg(value_enum, default_value_t = Suite::All)]
        suite: Suite,
        #[command(flatten)]
        on: On,
        #[arg(last = true)]
        args: Vec<String>,
    },
    /// Install system and Python dependencies; --check only diagnoses.
    Setup {
        #[arg(long)]
        check: bool,
        #[command(flatten)]
        on: On,
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
        #[arg(long, hide = true)]
        role: Option<Role>,
        #[arg(long, hide = true)]
        no_archive: bool,
    },
    /// Cross-compile for a host that packages and signs the result with `deploy --prebuilt`.
    #[command(hide = true)]
    Prebuild {
        #[arg(long)]
        triple: String,
        #[arg(long)]
        output: PathBuf,
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
        #[command(flatten)]
        on: On,
        #[command(flatten)]
        to: To,
        /// Reactivate and place the previous or given installed version.
        #[arg(long, value_name = "ID", num_args = 0..=1, conflicts_with = "bundle")]
        rollback: Option<Option<String>>,
        #[arg(long, hide = true)]
        role: Option<Role>,
        #[arg(long, hide = true, conflicts_with_all = ["bundle", "rollback"])]
        prebuilt: Option<PathBuf>,
        #[arg(long)]
        bundle: Option<PathBuf>,
        /// macOS application bundle or Linux desktop entry to replace.
        #[arg(long, env = "WEZ_VTABS_APP")]
        app: Option<PathBuf>,
        /// Directory for the bundled binary links; default ~/.local/bin.
        #[arg(long, env = "WEZ_VTABS_BIN")]
        bin: Option<PathBuf>,
        /// Install only; overrides --app and --bin.
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
    /// Active, pending and previous versions.
    Status {
        #[command(flatten)]
        to: To,
    },
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
    /// Release timings and allocation counts.
    Bench,
}

#[derive(Debug, Clone, Default, clap::Args)]
pub struct On {
    /// Machine that runs it: local or a targets.toml name; default `on` from targets.toml.
    #[arg(long, value_name = "MACHINE", add = ArgValueCandidates::new(crate::targets::candidates))]
    pub on: Option<String>,
}

#[derive(Debug, Clone, Default, clap::Args)]
pub struct To {
    /// Machine it is for: local (default) or a targets.toml name.
    #[arg(long, value_name = "MACHINE", add = ArgValueCandidates::new(crate::targets::candidates))]
    pub to: Option<String>,
}

#[derive(Debug, Parser, Default, Clone)]
pub struct BuildArgs {
    #[command(flatten)]
    pub on: On,
    #[command(flatten)]
    pub to: To,
    #[arg(long, hide = true)]
    pub role: Option<Role>,
    /// Container platform: distro[-version], e.g. ubuntu-26.04, arch.
    #[arg(long, hide = true)]
    pub platform: Option<String>,
    #[arg(long, hide = true, requires = "platform")]
    pub image: Option<String>,
    #[arg(long, hide = true)]
    pub container_runtime: Option<String>,
    #[arg(long, hide = true)]
    pub clean_builder: bool,
    #[arg(long, hide = true)]
    pub output: Option<PathBuf>,
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
