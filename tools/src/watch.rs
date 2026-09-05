use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus};
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};

use crate::{
    build, bundle, install, source,
    state::{BuildMetadata, Context, Lock, read_json},
};

struct Session {
    child: Child,
    finished: bool,
}

impl Session {
    fn try_wait(&mut self) -> Result<Option<ExitStatus>> {
        let status = self.child.try_wait()?;
        self.finished |= status.is_some();
        Ok(status)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // Once reaped, the child's PID may be reused. Never send a group signal
        // after observing that the child exited.
        if self.finished || self.child.try_wait().ok().flatten().is_some() {
            return;
        }
        #[cfg(unix)]
        unsafe {
            libc::kill(-(self.child.id() as i32), libc::SIGKILL);
        }
        #[cfg(windows)]
        {
            let _ = Command::new("taskkill")
                .args(["/PID", &self.child.id().to_string(), "/T", "/F"])
                .output();
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct Runtime {
    path: PathBuf,
    // Acquired before releasing build.lock, so GC cannot remove the runtime
    // between packaging, launching, and the end of the GUI's lifetime.
    _lease: fs::File,
    metadata: BuildMetadata,
}

fn start(ctx: &Context, path: &Path, args: &[String]) -> Result<Session> {
    let mut command = Command::new(install::gui_path(path));
    command
        .args(if args.is_empty() {
            vec!["start".into(), "--always-new-process".into()]
        } else {
            args.to_vec()
        })
        .env("WEZ_VTABS_BUNDLE", path)
        .current_dir(&ctx.root);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    Ok(Session {
        child: command.spawn()?,
        finished: false,
    })
}

fn fingerprint(ctx: &Context) -> Result<String> {
    let mut bytes = source::compile_digest(&ctx.root)?.into_bytes();
    for entry in walkdir::WalkDir::new(ctx.root.join("plugin")).sort_by_file_name() {
        let entry = entry?;
        if entry.file_type().is_file() {
            bytes.extend(
                entry
                    .path()
                    .strip_prefix(&ctx.root)?
                    .to_string_lossy()
                    .as_bytes(),
            );
            bytes.push(0);
            bytes.extend(fs::read(entry.path())?);
            bytes.push(0);
        }
    }
    Ok(crate::state::hash_bytes(&bytes))
}

fn build_runtime(ctx: &Context, previous: Option<&BuildMetadata>) -> Result<Runtime> {
    let _build_lock = Lock::acquire(&ctx.cache.join("build.lock"))?;
    let refreshed = match previous {
        Some(previous) => build::restage(ctx, previous)?,
        None => None,
    };
    let metadata = match refreshed {
        Some(metadata) => metadata,
        None => build::build(ctx)?,
    };
    let path = bundle::package(ctx, &metadata, &ctx.cache.join("bundles"), false)?;
    let name = path
        .file_name()
        .context("bundle name missing")?
        .to_string_lossy();
    let leases = ctx.cache.join("leases");
    fs::create_dir_all(&leases)?;
    let lease = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(leases.join(format!("{name}.lock")))?;
    fs2::FileExt::lock_shared(&lease)?;
    Ok(Runtime {
        path,
        _lease: lease,
        metadata,
    })
}

pub fn dev(ctx: &Context, watch: bool, debounce_ms: u64, args: &[String]) -> Result<i32> {
    let mut ctx = ctx.clone();
    if ctx.upstream.is_none() {
        ctx.upstream =
            read_json::<BuildMetadata>(&ctx.cache.join("build.json"))?.map(|build| build.upstream);
    }
    let mut previous = fingerprint(&ctx)?;
    let mut runtime = build_runtime(&ctx, None)?;
    // A first-ever watch session also pins its resolved revision; subsequent
    // edits do not each refetch upstream.
    ctx.upstream = Some(runtime.metadata.upstream.clone());
    let mut session = start(&ctx, &runtime.path, args)?;
    let mut pending = None;
    loop {
        if ctx.runner.cancelled() {
            return Ok(130);
        }
        if let Some(status) = session.try_wait()? {
            return Ok(status.code().unwrap_or(1));
        }
        if watch {
            let current = fingerprint(&ctx)?;
            if current != previous {
                previous = current;
                pending = Some(Instant::now());
            }
            if pending.is_some_and(|started: Instant| {
                started.elapsed() >= Duration::from_millis(debounce_ms)
            }) {
                pending = None;
                match build_runtime(&ctx, Some(&runtime.metadata)) {
                    Ok(next) => match start(&ctx, &next.path, args) {
                        Ok(next_session) => {
                            // A failed build or failed GUI spawn leaves the
                            // current session and its lease intact.
                            drop(session);
                            session = next_session;
                            runtime = next;
                        }
                        Err(error) => eprintln!("watch: {error:#}; current GUI kept running"),
                    },
                    Err(error) => {
                        eprintln!("watch: {error:#}; waiting for source changes");
                    }
                }
            }
        }
        // Keep the runtime lease alive through the current GUI's lifetime.
        let _ = &runtime;
        std::thread::sleep(Duration::from_millis(100));
    }
}
