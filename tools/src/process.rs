use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::state::{now, write_json};

#[derive(Clone, Debug)]
pub struct CommandSpec {
    pub program: OsString,
    pub args: Vec<OsString>,
    pub cwd: PathBuf,
    pub env: BTreeMap<OsString, OsString>,
    pub timeout: Option<Duration>,
}

impl CommandSpec {
    pub fn new(program: impl AsRef<OsStr>) -> Self {
        Self {
            program: program.as_ref().into(),
            args: Vec::new(),
            cwd: std::env::current_dir().unwrap_or_default(),
            env: BTreeMap::new(),
            timeout: None,
        }
    }
    pub fn arg(mut self, arg: impl AsRef<OsStr>) -> Self {
        self.args.push(arg.as_ref().into());
        self
    }
    pub fn args(mut self, args: impl IntoIterator<Item = impl AsRef<OsStr>>) -> Self {
        self.args
            .extend(args.into_iter().map(|arg| arg.as_ref().into()));
        self
    }
    pub fn cwd(mut self, cwd: impl AsRef<Path>) -> Self {
        self.cwd = cwd.as_ref().into();
        self
    }
    pub fn env(mut self, key: impl AsRef<OsStr>, value: impl AsRef<OsStr>) -> Self {
        self.env.insert(key.as_ref().into(), value.as_ref().into());
        self
    }
    pub fn timeout(mut self, duration: Duration) -> Self {
        self.timeout = Some(duration);
        self
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CommandRecord {
    pub command: Vec<String>,
    pub cwd: PathBuf,
    pub environment: BTreeMap<String, String>,
    pub stdout: PathBuf,
    pub stderr: PathBuf,
    pub duration_ms: u64,
    pub status: Option<i32>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RunReport {
    pub version: u32,
    pub invocation: Vec<String>,
    pub project_root: PathBuf,
    pub started_at: u64,
    pub duration_ms: u64,
    pub status: String,
    pub error: Option<String>,
    pub configuration: BTreeMap<String, String>,
    pub metadata: BTreeMap<String, Value>,
    pub commands: Vec<CommandRecord>,
    #[serde(default)]
    pub stages: Vec<StageRecord>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct StageRecord {
    pub name: String,
    pub duration_ms: u64,
}

pub struct Stage {
    runner: Runner,
    name: String,
    started: Instant,
}

impl Drop for Stage {
    fn drop(&mut self) {
        self.runner
            .0
            .report
            .lock()
            .unwrap()
            .stages
            .push(StageRecord {
                name: self.name.clone(),
                duration_ms: self.started.elapsed().as_millis() as u64,
            });
        let _ = self.runner.save();
    }
}

struct Inner {
    dir: PathBuf,
    report: Mutex<RunReport>,
    next: AtomicUsize,
    started: Instant,
    cancelled: AtomicBool,
}

#[derive(Clone)]
pub struct Runner(Arc<Inner>);

pub fn relevant_environment() -> BTreeMap<String, String> {
    std::env::vars()
        .filter(|(key, _)| relevant_key(key))
        .collect()
}

fn relevant_key(key: &str) -> bool {
    matches!(
        key,
        "RUSTFLAGS"
            | "CARGO_ENCODED_RUSTFLAGS"
            | "RUSTDOCFLAGS"
            | "RUSTUP_TOOLCHAIN"
            | "RUSTC"
            | "RUSTC_WRAPPER"
            | "CARGO_BUILD_TARGET"
            | "CARGO_INCREMENTAL"
            | "CC"
            | "CXX"
            | "AR"
            | "CFLAGS"
            | "CXXFLAGS"
            | "LDFLAGS"
            | "SDKROOT"
            | "MACOSX_DEPLOYMENT_TARGET"
            | "WEZ_VTABS_PROJECT_BRANCH"
            | "WEZ_VTABS_PROJECT_URL"
            | "WEZ_VTABS_UPSTREAM_URL"
            | "GIT_ALLOW_PROTOCOL"
            | "GIT_NO_LAZY_FETCH"
            | "GIT_TERMINAL_PROMPT"
    ) || key.starts_with("CARGO_PROFILE_")
        || key.starts_with("CARGO_TARGET_")
}

impl Runner {
    pub fn new(cache: &Path, root: &Path, invocation: Vec<String>) -> Result<Self> {
        let parent = cache.join("runs");
        fs::create_dir_all(&parent)?;
        let dir = tempfile::Builder::new()
            .prefix(&format!("{}-{}-", now(), std::process::id()))
            .tempdir_in(parent)?
            .keep();
        fs::create_dir(dir.join("commands"))?;
        let runner = Self(Arc::new(Inner {
            dir,
            report: Mutex::new(RunReport {
                version: 1,
                invocation,
                project_root: root.into(),
                started_at: now(),
                duration_ms: 0,
                status: "running".into(),
                error: None,
                configuration: relevant_environment(),
                metadata: BTreeMap::new(),
                commands: Vec::new(),
                stages: Vec::new(),
            }),
            next: AtomicUsize::new(0),
            started: Instant::now(),
            cancelled: AtomicBool::new(false),
        }));
        runner.save()?;
        Ok(runner)
    }

    pub fn directory(&self) -> &Path {
        &self.0.dir
    }
    pub fn stage(&self, name: &str) -> Stage {
        Stage {
            runner: self.clone(),
            name: name.into(),
            started: Instant::now(),
        }
    }
    pub fn cancel(&self) {
        self.0.cancelled.store(true, Ordering::SeqCst);
    }
    pub fn cancelled(&self) -> bool {
        self.0.cancelled.load(Ordering::SeqCst)
    }
    pub fn metadata(&self, key: &str, value: &Value) -> Result<()> {
        self.0
            .report
            .lock()
            .unwrap()
            .metadata
            .insert(key.into(), value.clone());
        self.save()
    }
    pub fn report(&self) -> RunReport {
        self.0.report.lock().unwrap().clone()
    }
    pub fn finish(&self, error: Option<String>) -> Result<()> {
        {
            let mut report = self.0.report.lock().unwrap();
            report.duration_ms = self.0.started.elapsed().as_millis() as u64;
            report.status = if error.is_some() { "failed" } else { "passed" }.into();
            report.error = error;
        }
        self.save()
    }
    fn save(&self) -> Result<()> {
        write_json(
            &self.0.dir.join("run.json"),
            &*self.0.report.lock().unwrap(),
        )
    }
    pub fn run(&self, spec: CommandSpec) -> Result<()> {
        self.execute(spec, false).map(|_| ())
    }
    pub fn capture(&self, spec: CommandSpec) -> Result<String> {
        self.execute(spec, true)
    }

    fn execute(&self, spec: CommandSpec, capture: bool) -> Result<String> {
        ensure!(!self.cancelled(), "operation cancelled");
        let index = self.0.next.fetch_add(1, Ordering::SeqCst);
        let stdout_path = PathBuf::from(format!("commands/{index:04}.stdout.log"));
        let stderr_path = PathBuf::from(format!("commands/{index:04}.stderr.log"));
        let command: Vec<String> = std::iter::once(&spec.program)
            .chain(&spec.args)
            .map(|s| s.to_string_lossy().into_owned())
            .collect();
        if !capture {
            eprintln!("+ {}", command.join(" "));
        }
        let start = Instant::now();
        let mut record = CommandRecord {
            command,
            cwd: spec.cwd.clone(),
            environment: spec
                .env
                .iter()
                .filter_map(|(k, v)| {
                    let key = k.to_string_lossy();
                    relevant_key(&key).then(|| (key.into_owned(), v.to_string_lossy().into_owned()))
                })
                .collect(),
            stdout: stdout_path.clone(),
            stderr: stderr_path.clone(),
            duration_ms: 0,
            status: None,
            error: None,
        };
        self.0.report.lock().unwrap().commands.push(record.clone());
        self.save()?;
        let result = (|| -> Result<(std::process::ExitStatus, String)> {
            let stdout_file = File::create(self.0.dir.join(stdout_path))?;
            let stderr_file = File::create(self.0.dir.join(stderr_path))?;
            let mut command = Command::new(&spec.program);
            command
                .args(&spec.args)
                .current_dir(&spec.cwd)
                .envs(&spec.env)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            #[cfg(unix)]
            {
                use std::os::unix::process::CommandExt;
                command.process_group(0);
            }
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                command.creation_flags(0x08000000);
            }
            let mut child = command
                .spawn()
                .with_context(|| format!("start: {}", spec.program.to_string_lossy()))?;
            let stdout = child.stdout.take().context("stdout missing")?;
            let stderr = child.stderr.take().context("stderr missing")?;
            let out = std::thread::spawn(move || pump(stdout, stdout_file, !capture, capture));
            let err = std::thread::spawn(move || pump(stderr, stderr_file, true, false));
            let mut interrupted = None;
            let status = loop {
                if let Some(status) = child.try_wait()? {
                    break status;
                }
                if self.cancelled() || spec.timeout.is_some_and(|limit| start.elapsed() >= limit) {
                    interrupted = Some(if self.cancelled() {
                        "operation cancelled"
                    } else {
                        "command timed out"
                    });
                    #[cfg(unix)]
                    unsafe {
                        libc::kill(-(child.id() as i32), libc::SIGKILL);
                    }
                    #[cfg(windows)]
                    {
                        let _ = Command::new("taskkill")
                            .args(["/PID", &child.id().to_string(), "/T", "/F"])
                            .stdout(Stdio::null())
                            .stderr(Stdio::null())
                            .status();
                    }
                    let _ = child.kill();
                    break child.wait()?;
                }
                std::thread::sleep(Duration::from_millis(20));
            };
            let draining = Instant::now();
            let mut stopped = false;
            while !out.is_finished() || !err.is_finished() {
                if !stopped
                    && (self.cancelled()
                        || spec.timeout.is_some_and(|limit| start.elapsed() >= limit)
                        || draining.elapsed() >= Duration::from_secs(2))
                {
                    interrupted = Some(if self.cancelled() {
                        "operation cancelled"
                    } else {
                        "subprocess retained output streams"
                    });
                    #[cfg(unix)]
                    unsafe {
                        libc::kill(-(child.id() as i32), libc::SIGKILL);
                    }
                    #[cfg(windows)]
                    {
                        let _ = Command::new("taskkill")
                            .args(["/PID", &child.id().to_string(), "/T", "/F"])
                            .stdout(Stdio::null())
                            .stderr(Stdio::null())
                            .status();
                    }
                    stopped = true;
                }
                if stopped && draining.elapsed() >= Duration::from_secs(3) {
                    bail!(
                        "{}",
                        interrupted.unwrap_or("subprocess output did not close")
                    );
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            let output = out
                .join()
                .map_err(|_| anyhow::anyhow!("stdout worker failed"))??;
            err.join()
                .map_err(|_| anyhow::anyhow!("stderr worker failed"))??;
            if let Some(reason) = interrupted {
                bail!("{reason}");
            }
            Ok((
                status,
                String::from_utf8(output).context("command output is not UTF-8")?,
            ))
        })();
        record.duration_ms = start.elapsed().as_millis() as u64;
        match &result {
            Ok((status, _)) => {
                record.status = status.code();
                if !status.success() {
                    record.error = Some(format!("exit: {status}"));
                }
            }
            Err(error) => record.error = Some(format!("{error:#}")),
        }
        {
            let mut report = self.0.report.lock().unwrap();
            let pending = report
                .commands
                .iter_mut()
                .find(|pending| pending.stdout == record.stdout)
                .context("command record missing")?;
            *pending = record;
        }
        self.save()?;
        let (status, output) = result?;
        ensure!(
            status.success(),
            "command failed ({status}): {}",
            spec.program.to_string_lossy()
        );
        Ok(output.trim_end_matches(['\r', '\n']).into())
    }

    pub fn timings(&self) -> Value {
        let report = self.report();
        json!({"duration_ms":report.duration_ms,"stages":report.stages,"commands":report.commands.iter().map(|c| json!({"command":c.command,"duration_ms":c.duration_ms,"status":c.status})).collect::<Vec<_>>()})
    }
}

fn pump(mut input: impl Read, mut file: File, display: bool, capture: bool) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        let count = input.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        file.write_all(&buffer[..count])?;
        if capture {
            output.extend_from_slice(&buffer[..count]);
        }
        if display {
            let _ = std::io::stderr().write_all(&buffer[..count]);
        }
    }
    file.flush()?;
    Ok(output)
}
