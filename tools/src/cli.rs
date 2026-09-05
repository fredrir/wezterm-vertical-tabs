use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "wez-vtabs",
    version,
    about = "Build, validate and manage native WezTerm bundles"
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
    /// Resolve upstream and synchronize the native checkout.
    Prepare,
    /// Run the selected upstream system dependency installer.
    Deps,
    /// Compile and validate native binaries.
    Build,
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
    Native,
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
