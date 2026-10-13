use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};
use notify::event::{MetadataKind, ModifyKind};
use notify::{EventKind, RecursiveMode};
use notify_debouncer_full::{DebounceEventResult, new_debouncer};
use process_wrap::std::ChildWrapper;

use crate::source::inventory;
use crate::state::{self, BuildMetadata, Context, Lock};
use crate::{build, bundle, install, process};

const WORKSPACE: &str = "vtabs-dev";
const FRAME: &str = "window_frame={active_titlebar_bg=\"#b45309\",active_titlebar_fg=\"#ffffff\"}";

struct Session(Box<dyn ChildWrapper>);

impl Session {
    fn try_wait(&mut self) -> Result<Option<ExitStatus>> {
        Ok(self.0.try_wait()?)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // A reaped child's PID may be reused, so never signal its group after it exited.
        if matches!(self.0.try_wait(), Ok(None)) {
            let _ = self.0.kill();
        }
    }
}

struct Runtime {
    path: PathBuf,
    // Taken before build.lock is released so GC keeps the runtime while the GUI runs.
    _lease: Lock,
    metadata: BuildMetadata,
}

/// GUI arguments: dev configuration unless overridden, then `start` in a new process.
fn gui_arguments(args: &[String]) -> Vec<String> {
    if args
        .iter()
        .any(|arg| matches!(arg.as_str(), "--version" | "-V" | "--help" | "-h"))
    {
        return args.to_vec();
    }
    let configures = |arg: &String| {
        matches!(arg.as_str(), "-n" | "--skip-config") || arg.starts_with("--config")
    };
    let mut global: Vec<String> = if args.iter().any(configures) {
        Vec::new()
    } else {
        vec!["--skip-config".into(), "--config".into(), FRAME.into()]
    };
    let mut start = Vec::new();
    let mut rest = args.iter();
    if args.iter().any(|arg| arg == "start") {
        global.extend(rest.by_ref().take_while(|arg| *arg != "start").cloned());
        start.extend(rest.cloned());
    } else {
        while let Some(arg) = rest.next() {
            if configures(arg) {
                global.push(arg.clone());
                if matches!(arg.as_str(), "--config" | "--config-file") {
                    global.extend(rest.next().cloned());
                }
            } else {
                start.push(arg.clone());
            }
        }
        if !start
            .iter()
            .any(|arg| arg == "--workspace" || arg.starts_with("--workspace="))
        {
            start.splice(0..0, ["--workspace".to_owned(), WORKSPACE.to_owned()]);
        }
    }
    for flag in ["--no-auto-connect", "--always-new-process"] {
        if !start.iter().any(|arg| arg == flag) {
            start.insert(0, flag.into());
        }
    }
    global.push("start".into());
    global.extend(start);
    global
}

fn start(ctx: &Context, bundle: &Path, args: &[String]) -> Result<Session> {
    let mut command = Command::new(install::gui_path(bundle));
    for key in [
        "WEZTERM_UNIX_SOCKET",
        "WEZTERM_PANE",
        "WEZTERM_CONFIG_FILE",
        "WEZTERM_CONFIG_DIR",
        "WEZTERM_EXECUTABLE",
        "WEZTERM_EXECUTABLE_DIR",
        "WEZTERM_SHELL_SKIP_SEMANTIC_ZONES",
        "WEZTERM_SHELL_SKIP_CWD",
    ] {
        command.env_remove(key);
    }
    command
        .args(gui_arguments(args))
        .env("WEZ_VTABS_BUNDLE", bundle)
        .env("WEZ_VTABS_DEV", "1")
        .current_dir(&ctx.root);
    Ok(Session(process::spawn_group(command)?))
}

#[cfg(target_os = "macos")]
mod macos_window {
    use std::ffi::c_void;
    use std::time::{Duration, Instant};

    type CFArrayRef = *const c_void;
    type CFDictionaryRef = *const c_void;
    type CFStringRef = *const c_void;
    type CFNumberRef = *const c_void;
    type CFIndex = libc::c_long;
    type Boolean = libc::c_uchar;

    pub fn wait_for_window(pid: u32, timeout: Duration) {
        let start = Instant::now();
        while start.elapsed() < timeout {
            if has_window(pid) {
                std::thread::sleep(Duration::from_millis(60));
                return;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    fn has_window(target_pid: u32) -> bool {
        unsafe {
            let cg_list_copy: Option<unsafe extern "C" fn(u32, u32) -> CFArrayRef> =
                std::mem::transmute(libc::dlsym(
                    libc::RTLD_DEFAULT,
                    c"CGWindowListCopyWindowInfo".as_ptr(),
                ));
            let cf_count: Option<unsafe extern "C" fn(CFArrayRef) -> CFIndex> =
                std::mem::transmute(libc::dlsym(libc::RTLD_DEFAULT, c"CFArrayGetCount".as_ptr()));
            let cf_val_at: Option<unsafe extern "C" fn(CFArrayRef, CFIndex) -> *const c_void> =
                std::mem::transmute(libc::dlsym(
                    libc::RTLD_DEFAULT,
                    c"CFArrayGetValueAtIndex".as_ptr(),
                ));
            let cf_dict_val: Option<
                unsafe extern "C" fn(CFDictionaryRef, *const c_void) -> *const c_void,
            > = std::mem::transmute(libc::dlsym(
                libc::RTLD_DEFAULT,
                c"CFDictionaryGetValue".as_ptr(),
            ));
            let cf_num_val: Option<
                unsafe extern "C" fn(CFNumberRef, libc::c_int, *mut libc::c_int) -> Boolean,
            > = std::mem::transmute(libc::dlsym(
                libc::RTLD_DEFAULT,
                c"CFNumberGetValue".as_ptr(),
            ));
            let cf_str_create: Option<
                unsafe extern "C" fn(*const c_void, *const libc::c_char, u32) -> CFStringRef,
            > = std::mem::transmute(libc::dlsym(
                libc::RTLD_DEFAULT,
                c"CFStringCreateWithCString".as_ptr(),
            ));
            let cf_release: Option<unsafe extern "C" fn(*const c_void)> =
                std::mem::transmute(libc::dlsym(libc::RTLD_DEFAULT, c"CFRelease".as_ptr()));

            let (
                Some(cg_list_copy),
                Some(cf_count),
                Some(cf_val_at),
                Some(cf_dict_val),
                Some(cf_num_val),
                Some(cf_str_create),
                Some(cf_release),
            ) = (
                cg_list_copy,
                cf_count,
                cf_val_at,
                cf_dict_val,
                cf_num_val,
                cf_str_create,
                cf_release,
            )
            else {
                return false;
            };

            let k_pid = cf_str_create(std::ptr::null(), c"kCGWindowOwnerPID".as_ptr(), 0x08000100);
            let k_layer = cf_str_create(std::ptr::null(), c"kCGWindowLayer".as_ptr(), 0x08000100);

            let windows = cg_list_copy(1, 0);
            let count = if !windows.is_null() {
                cf_count(windows)
            } else {
                0
            };
            let mut found = false;

            for i in 0..count {
                let w = cf_val_at(windows, i);
                let pid_val = cf_dict_val(w, k_pid);
                let mut pid_out: libc::c_int = 0;
                if !pid_val.is_null()
                    && cf_num_val(pid_val, 9, &mut pid_out) != 0
                    && pid_out as u32 == target_pid
                {
                    let layer_val = cf_dict_val(w, k_layer);
                    let mut layer_out: libc::c_int = 0;
                    if !layer_val.is_null()
                        && cf_num_val(layer_val, 9, &mut layer_out) != 0
                        && layer_out == 0
                    {
                        found = true;
                        break;
                    }
                }
            }

            if !windows.is_null() {
                cf_release(windows);
            }
            if !k_pid.is_null() {
                cf_release(k_pid);
            }
            if !k_layer.is_null() {
                cf_release(k_layer);
            }

            found
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod macos_window {
    use std::time::Duration;
    pub fn wait_for_window(_pid: u32, _timeout: Duration) {}
}

/// Project paths whose edits change the dev bundle: compile inputs and the Lua plugin.
fn reloads(root: &Path, path: &Path) -> Option<PathBuf> {
    let relative = path.strip_prefix(root).ok()?;
    let name = relative.file_name()?.to_string_lossy();
    let temporary = name.starts_with('.')
        || name.ends_with('~')
        || name.ends_with(".swp")
        || name.ends_with(".tmp");
    let relevant = inventory::is_compile_input(relative) || relative.starts_with("plugin");
    (relevant && !temporary && !inventory::is_ignored(relative)).then(|| relative.to_path_buf())
}

fn content_change(kind: &EventKind) -> bool {
    match kind {
        EventKind::Access(_) => false,
        EventKind::Modify(ModifyKind::Metadata(metadata)) => {
            matches!(metadata, MetadataKind::WriteTime | MetadataKind::Any)
        }
        _ => true,
    }
}

#[derive(Default)]
struct Changes {
    paths: BTreeSet<PathBuf>,
    rescan: bool,
}

impl Changes {
    fn add(&mut self, root: &Path, result: DebounceEventResult) {
        match result {
            Ok(events) => {
                for event in events.iter().filter(|event| content_change(&event.kind)) {
                    self.rescan |= event.need_rescan();
                    self.paths
                        .extend(event.paths.iter().filter_map(|path| reloads(root, path)));
                }
            }
            Err(errors) => {
                for error in errors {
                    eprintln!("dev: watch: {error}");
                }
            }
        }
    }
}

fn build_runtime(ctx: &Context, previous: Option<&BuildMetadata>) -> Result<Runtime> {
    let _build_lock = ctx.lock_build()?;
    let refreshed = match previous {
        Some(previous) => build::restage(ctx, previous)?,
        None => None,
    };
    let metadata = match refreshed {
        Some(metadata) => metadata,
        None => build::build(ctx)?,
    };
    let path = bundle::package(ctx, &metadata, &ctx.cache.join("bundles"), false)?;
    let name = path.file_name().context("bundle name missing")?;
    let lease = Lock::shared(&state::lease(&ctx.cache, &name.to_string_lossy()))?;
    Ok(Runtime {
        path,
        _lease: lease,
        metadata,
    })
}

pub fn dev(ctx: &Context, watch: bool, debounce_ms: u64, args: &[String]) -> Result<i32> {
    let mut ctx = ctx.clone();
    if ctx.upstream.is_none() {
        ctx.upstream = ctx.last_build()?.map(|build| build.upstream);
    }
    eprintln!("dev: building");
    let build_start = Instant::now();
    let mut runtime = build_runtime(&ctx, None)?;
    // Pin the resolved revision so later rebuilds never refetch upstream.
    ctx.upstream = Some(runtime.metadata.upstream.clone());
    let mut session = start(&ctx, &runtime.path, args)?;
    eprintln!(
        "dev: GUI started in {:.2}s (pid {})",
        build_start.elapsed().as_secs_f32(),
        session.0.id()
    );

    let root = ctx.root.canonicalize()?;
    let (sender, receiver) = mpsc::channel();
    let debounce = Duration::from_millis(debounce_ms.max(1));
    let mut debouncer = new_debouncer(debounce, None, sender)?;
    if watch {
        debouncer.watch(&root, RecursiveMode::NonRecursive)?;
        for item in inventory::SOURCE_ITEMS
            .iter()
            .filter(|item| inventory::is_compile_input(Path::new(item)) || **item == "plugin")
        {
            let path = root.join(item);
            if path.is_dir() {
                debouncer.watch(&path, RecursiveMode::Recursive)?;
            }
        }
        eprintln!("dev: watching for changes");
    }
    loop {
        if ctx.runner.cancelled() {
            return Ok(130);
        }
        if let Some(status) = session.try_wait()? {
            return Ok(status.code().unwrap_or(1));
        }
        let mut changes = Changes::default();
        match receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(result) => changes.add(&root, result),
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => anyhow::bail!("watch stopped"),
        }
        if changes.paths.is_empty() && !changes.rescan {
            continue;
        }
        // Wait for a quiet period so multi-file writes rebuild once.
        while let Ok(result) = receiver.recv_timeout(debounce) {
            changes.add(&root, result);
        }
        let changed = changes.paths;
        let action = if !changes.rescan && changed.iter().all(|path| path.starts_with("plugin")) {
            "restaging"
        } else {
            "rebuilding"
        };
        let summary = match (changed.first(), changed.len()) {
            (None, _) => "sources".to_string(),
            (Some(first), 1) => first.display().to_string(),
            (Some(first), count) => format!("{} (+{} more)", first.display(), count - 1),
        };
        eprintln!("dev: {summary} changed; {action}");
        let reload_start = Instant::now();
        match build_runtime(&ctx, Some(&runtime.metadata)) {
            Ok(next) => match start(&ctx, &next.path, args) {
                Ok(next_session) => {
                    // Keep the previous window until the new one is on screen to avoid a flicker.
                    if std::env::var_os("WEZ_VTABS_PROJECT_URL").is_none() {
                        macos_window::wait_for_window(
                            next_session.0.id(),
                            Duration::from_millis(800),
                        );
                    }
                    session = next_session;
                    runtime = next;
                    eprintln!(
                        "dev: reloaded in {:.2}s",
                        reload_start.elapsed().as_secs_f32()
                    );
                }
                Err(error) => eprintln!("dev: GUI start failed; previous GUI kept: {error:#}"),
            },
            Err(error) => eprintln!("dev: build failed; previous GUI kept\n{error:#}"),
        }
    }
}
