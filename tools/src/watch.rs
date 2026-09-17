use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus};
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};

use crate::{
    build, bundle, install,
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

fn build_gui_command(gui: &Path, bundle: &Path, root: &Path, args: &[String]) -> Command {
    let mut command = Command::new(gui);
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
        .env("WEZ_VTABS_BUNDLE", bundle)
        .env("WEZ_VTABS_DEV", "1")
        .current_dir(root);

    let has_config_opt = args.iter().any(|arg| {
        arg == "-n"
            || arg == "--skip-config"
            || arg == "--config-file"
            || arg.starts_with("--config-file=")
            || arg == "--config"
            || arg.starts_with("--config=")
    });

    let is_info_command = args
        .iter()
        .any(|arg| arg == "--version" || arg == "-V" || arg == "--help" || arg == "-h");
    if is_info_command {
        command.args(args);
        return command;
    }

    let mut global_args: Vec<String> = Vec::new();
    let mut sub_args: Vec<String> = Vec::new();

    if !has_config_opt {
        global_args.push("--skip-config".into());
        global_args.push("--config".into());
        global_args.push("window_frame={active_titlebar_bg=\"#b45309\",active_titlebar_fg=\"#ffffff\"}".into());
    }

    let has_start = args.iter().any(|arg| arg == "start");
    if has_start {
        let mut past_start = false;
        for arg in args {
            if arg == "start" {
                past_start = true;
                sub_args.push(arg.clone());
            } else if !past_start {
                global_args.push(arg.clone());
            } else {
                sub_args.push(arg.clone());
            }
        }
        if !sub_args.iter().any(|a| a == "--always-new-process") {
            sub_args.push("--always-new-process".into());
        }
        if !sub_args.iter().any(|a| a == "--no-auto-connect") {
            sub_args.push("--no-auto-connect".into());
        }
    } else {
        for arg in args {
            if arg == "-n" || arg == "--skip-config" || arg.starts_with("--config") {
                global_args.push(arg.clone());
            } else {
                sub_args.push(arg.clone());
            }
        }
        global_args.push("start".into());
        global_args.push("--always-new-process".into());
        global_args.push("--no-auto-connect".into());
        if !sub_args.iter().any(|a| a == "--workspace" || a.starts_with("--workspace=")) {
            global_args.push("--workspace".into());
            global_args.push("vtabs-dev".into());
        }
    }

    command.args(global_args);
    command.args(sub_args);

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    command
}

fn start(ctx: &Context, path: &Path, args: &[String]) -> Result<Session> {
    let gui_binary = install::gui_path(path);
    let mut command = build_gui_command(&gui_binary, path, &ctx.root, args);
    Ok(Session {
        child: command.spawn()?,
        finished: false,
    })
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
            > = std::mem::transmute(libc::dlsym(libc::RTLD_DEFAULT, c"CFNumberGetValue".as_ptr()));
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

            let k_pid =
                cf_str_create(std::ptr::null(), c"kCGWindowOwnerPID".as_ptr(), 0x08000100);
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileStat {
    mtime: std::time::SystemTime,
    len: u64,
}

fn scan_source_stats(root: &Path) -> BTreeMap<PathBuf, FileStat> {
    let mut stats = BTreeMap::new();
    let dirs = ["src", "plugin", "wezterm-patches"];
    for dir_name in dirs {
        let dir = root.join(dir_name);
        if !dir.is_dir() {
            continue;
        }
        for entry in walkdir::WalkDir::new(&dir).follow_links(false) {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            if entry.file_type().is_file() {
                let file_name = entry.file_name().to_string_lossy();
                if file_name.starts_with('.')
                    || file_name.ends_with('~')
                    || file_name.ends_with(".swp")
                    || file_name.ends_with(".tmp")
                {
                    continue;
                }
                if let Ok(meta) = entry.metadata()
                    && let Ok(mtime) = meta.modified() {
                        let rel = entry.path().strip_prefix(root).unwrap_or(entry.path());
                        stats.insert(
                            rel.to_path_buf(),
                            FileStat {
                                mtime,
                                len: meta.len(),
                            },
                        );
                    }
            }
        }
    }
    for file_name in ["Cargo.toml", "Cargo.lock", "rust-toolchain.toml"] {
        let file = root.join(file_name);
        if let Ok(meta) = file.metadata()
            && let Ok(mtime) = meta.modified() {
                stats.insert(
                    PathBuf::from(file_name),
                    FileStat {
                        mtime,
                        len: meta.len(),
                    },
                );
            }
    }
    stats
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
    eprintln!("[vtabs:dev] 🚀 Initializing clean dev build...");
    let build_start = Instant::now();
    let mut runtime = build_runtime(&ctx, None)?;
    // A first-ever watch session also pins its resolved revision; subsequent
    // edits do not each refetch upstream.
    ctx.upstream = Some(runtime.metadata.upstream.clone());
    let mut session = start(&ctx, &runtime.path, args)?;
    let build_elapsed = build_start.elapsed().as_secs_f32();
    let pid = session.child.id();
    eprintln!("[vtabs:dev] ✨ GUI ready in {build_elapsed:.2}s (pid: {pid}, workspace: vtabs-dev)");
    if watch {
        eprintln!("[vtabs:dev] ⏳ Hot reload active (watching src/ and plugin/ with {debounce_ms}ms debounce)");
        eprintln!("[vtabs:dev] 💡 Edit any Lua or Rust file to hot-reload. Press Ctrl+C to stop.\n");
    }

    let mut previous_stats = scan_source_stats(&ctx.root);
    let mut pending: Option<(Instant, Vec<String>)> = None;
    loop {
        if ctx.runner.cancelled() {
            return Ok(130);
        }
        if let Some(status) = session.try_wait()? {
            return Ok(status.code().unwrap_or(1));
        }
        if watch {
            let current_stats = scan_source_stats(&ctx.root);
            if current_stats != previous_stats {
                let mut changed = Vec::new();
                for (path, stat) in &current_stats {
                    if previous_stats.get(path) != Some(stat) {
                        changed.push(path.display().to_string());
                    }
                }
                for path in previous_stats.keys() {
                    if !current_stats.contains_key(path) {
                        changed.push(format!("{} (deleted)", path.display()));
                    }
                }
                previous_stats = current_stats;
                pending = Some((Instant::now(), changed));
            }
            if let Some((started, changed)) = &pending
                && started.elapsed() >= Duration::from_millis(debounce_ms) {
                    let changed_files = changed.clone();
                    pending = None;

                    let is_lua_only = changed_files.iter().all(|p| p.starts_with("plugin/"));
                    let action = if is_lua_only { "restaging" } else { "rebuilding" };
                    let summary = if changed_files.is_empty() {
                        "source".to_string()
                    } else if changed_files.len() <= 2 {
                        changed_files.join(", ")
                    } else {
                        format!("{} (+{} more)", changed_files[0], changed_files.len() - 1)
                    };
                    eprintln!("[vtabs:dev] ⚡ Change detected in {summary} ({action}...)");

                    let reload_start = Instant::now();
                    match build_runtime(&ctx, Some(&runtime.metadata)) {
                        Ok(next) => match start(&ctx, &next.path, args) {
                            Ok(next_session) => {
                                // Seamless zero-flicker handoff: wait for the new window to be
                                // mapped and rendered before dropping the previous session.
                                if std::env::var_os("WEZ_VTABS_PROJECT_URL").is_none() {
                                    macos_window::wait_for_window(
                                        next_session.child.id(),
                                        Duration::from_millis(800),
                                    );
                                }
                                drop(session);
                                session = next_session;
                                runtime = next;
                                let elapsed = reload_start.elapsed().as_secs_f32();
                                eprintln!("[vtabs:dev] ✨ Reloaded GUI in {elapsed:.2}s\n");
                            }
                            Err(error) => {
                                eprintln!(
                                    "[vtabs:dev] ❌ GUI spawn failed: {error:#}; current GUI kept running\n"
                                );
                            }
                        },
                        Err(error) => {
                            eprintln!(
                                "[vtabs:dev] ❌ Build failed:\n{error:#}\n[vtabs:dev] ⏳ Current GUI kept running; waiting for changes...\n"
                            );
                        }
                    }
                }
        }
        // Keep the runtime lease alive through the current GUI's lifetime.
        let _ = &runtime;
        std::thread::sleep(Duration::from_millis(100));
    }
}
