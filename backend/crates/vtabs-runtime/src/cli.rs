//! `wezterm cli` from inside the pane. This process runs where the pane lives, with the server's
//! socket and its own pane id in the environment, so it can kill and move panes there that the
//! GUI's mux client cannot reach. Every call passes `--no-auto-start`: a cli that finds no server
//! must never spawn one over the socket path.

use std::ffi::OsStr;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use serde_json::Value;

const TIMEOUT: Duration = Duration::from_secs(5);
const OUTPUT_MAX: usize = 1024 * 1024;
const PIPE_CLOSE_GRACE: Duration = Duration::from_millis(250);
#[cfg(windows)]
const BIN: &str = "wezterm.exe";
#[cfg(not(windows))]
const BIN: &str = "wezterm";

fn is_mux_socket(socket: Option<&OsStr>) -> bool {
    socket
        .and_then(|value| Path::new(value).file_name())
        .is_some_and(|name| !name.to_string_lossy().starts_with("gui-sock-"))
}

struct Captured {
    bytes: Vec<u8>,
    truncated: bool,
}

fn drain<R: Read + Send + 'static>(mut pipe: R) -> Receiver<io::Result<Captured>> {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let result = (|| {
            let mut bytes = Vec::new();
            let mut buffer = [0; 8192];
            let mut truncated = false;
            loop {
                let read = pipe.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                let retained = read.min(OUTPUT_MAX.saturating_sub(bytes.len()));
                bytes.extend_from_slice(&buffer[..retained]);
                truncated |= retained < read;
            }
            Ok(Captured { bytes, truncated })
        })();
        let _ = sender.send(result);
    });
    receiver
}

fn drained(reader: Receiver<io::Result<Captured>>, deadline: Instant) -> Result<Captured, String> {
    reader
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .map_err(|err| match err {
            mpsc::RecvTimeoutError::Timeout => "cli output pipe did not close".to_string(),
            mpsc::RecvTimeoutError::Disconnected => "cli output reader stopped".to_string(),
        })?
        .map_err(|e| e.to_string())
}

/// Reads both pipes while the child runs. Waiting before reading can deadlock once either pipe
/// fills. Capture is bounded while excess bytes are still drained. On timeout the direct child is
/// killed and reaped, but the call never waits indefinitely for pipe handles inherited by a
/// descendant. `input`, when given, is the child's whole stdin.
fn run_command(
    command: &mut Command,
    timeout: Duration,
    label: &str,
    input: Option<&[u8]>,
) -> Result<Output, String> {
    let stdin = if input.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    };
    let mut child = command
        .stdin(stdin)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("{label}: {e}"))?;
    if let Some(input) = input
        && let Some(mut stdin) = child.stdin.take()
    {
        // A few bytes fit the pipe whole; a child that exits first explains itself on stderr.
        let _ = stdin.write_all(input);
    }
    let stdout = drain(child.stdout.take().expect("stdout was piped"));
    let stderr = drain(child.stderr.take().expect("stderr was piped"));
    let started = Instant::now();
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if started.elapsed() < timeout => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Ok(None) => {
                timed_out = true;
                let _ = child.kill();
                break child.wait().map_err(|e| e.to_string());
            }
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(e.to_string());
            }
        }
    };
    let status = status?;
    if timed_out {
        return Err(format!("{label} timed out"));
    }
    let pipe_deadline = Instant::now() + PIPE_CLOSE_GRACE;
    let stdout = drained(stdout, pipe_deadline)?;
    let stderr = drained(stderr, pipe_deadline)?;
    if stdout.truncated || stderr.truncated {
        return Err(format!("{label}: output exceeded {OUTPUT_MAX} bytes"));
    }
    Ok(Output {
        status,
        stdout: stdout.bytes,
        stderr: stderr.bytes,
    })
}

/// The fields of `wezterm cli list --format json` the verbs read.
#[derive(Debug, Clone, PartialEq)]
pub struct PaneInfo {
    pub pane_id: u64,
    pub tab_id: u64,
    pub title: String,
    pub left_col: i64,
    pub cols: i64,
    pub is_active: bool,
}

/// `cli list --format json`, one entry per pane; malformed entries are skipped.
pub fn panes_from_json(json: &str) -> Result<Vec<PaneInfo>, String> {
    let list: Vec<Value> = serde_json::from_str(json).map_err(|e| format!("cli list: {e}"))?;
    let int = |v: &Value, key: &str| v.get(key).and_then(Value::as_i64);
    let uint = |v: &Value, key: &str| v.get(key).and_then(Value::as_u64);
    Ok(list
        .iter()
        .filter_map(|v| {
            let left_col = int(v, "left_col")?;
            let cols = v.get("size").and_then(|size| int(size, "cols"))?;
            if left_col < 0 || cols <= 0 || left_col.checked_add(cols).is_none() {
                return None;
            }
            Some(PaneInfo {
                pane_id: uint(v, "pane_id")?,
                tab_id: uint(v, "tab_id")?,
                title: v.get("title")?.as_str()?.to_string(),
                left_col,
                cols,
                is_active: v.get("is_active").and_then(Value::as_bool).unwrap_or(false),
            })
        })
        .collect())
}

/// Which pane must hold focus for an `adjust-pane-size` to reach the sidebar's own split, and the
/// pane displaced to make that so. The walk starts at the tab's active pane and stops at the first
/// horizontal split above it (mux/src/tab.rs `adjust_pane_size`): from the sidebar that is the
/// root; from a content pane it is the root only when the pane spans the whole content column,
/// since every horizontal split narrows its children and a vertical one never does.
/// `Ok(None)` when the active pane already reaches it, `Ok(Some(id))` naming the content pane to
/// hand focus back to once the sidebar has taken it.
pub fn adjust_plan(panes: &[PaneInfo], own: u64) -> Result<Option<u64>, String> {
    let me = panes
        .iter()
        .find(|p| p.pane_id == own)
        .ok_or_else(|| format!("own pane {own} not listed"))?;
    let tab: Vec<&PaneInfo> = panes.iter().filter(|p| p.tab_id == me.tab_id).collect();
    let tab_cols = tab.iter().map(|p| p.left_col + p.cols).max().unwrap_or(0);
    let content_width = tab_cols - me.cols - 1;
    let Some(active) = tab.iter().find(|p| p.is_active) else {
        return Err("no active pane in this tab".into());
    };
    if active.pane_id == own || active.cols == content_width {
        return Ok(None);
    }
    Ok(Some(active.pane_id))
}

/// What `adjust-pane-size` must do to put `own` at `target` columns, read from the server's own
/// pane list rather than a mirror that may lag it: `Ok(None)` when the pane is there already.
/// The target is held to what the tab can give, `min_content` for each band of content beside
/// the pane (panes sharing a left edge are one band, the way the plugin counts them); WezTerm's
/// own `[1, width-2]` clamp applies after that. `own` is the split's first child at the left
/// edge, where `Right` grows it, and its second child anywhere else, where `Left` does.
pub fn adjust_delta(
    panes: &[PaneInfo],
    own: u64,
    target: u32,
    min_content: u32,
) -> Result<Option<(&'static str, u32)>, String> {
    let me = panes
        .iter()
        .find(|p| p.pane_id == own)
        .ok_or_else(|| format!("own pane {own} not listed"))?;
    let tab: Vec<&PaneInfo> = panes.iter().filter(|p| p.tab_id == me.tab_id).collect();
    let tab_cols = tab.iter().map(|p| p.left_col + p.cols).max().unwrap_or(0);
    let mut lefts: Vec<i64> = tab
        .iter()
        .filter(|p| p.pane_id != own)
        .map(|p| p.left_col)
        .collect();
    lefts.sort_unstable();
    lefts.dedup();
    let bands = i64::try_from(lefts.len().max(1)).unwrap_or(1);
    let cap = (tab_cols - i64::from(min_content) * bands).max(1);
    let target = i64::from(target).clamp(1, cap);
    let delta = target - me.cols;
    if delta == 0 {
        return Ok(None);
    }
    let grows = if me.left_col == 0 { "Right" } else { "Left" };
    let shrinks = if grows == "Right" { "Left" } else { "Right" };
    let amount = u32::try_from(delta.unsigned_abs()).map_err(|_| "adjust: delta out of range")?;
    Ok(Some((if delta > 0 { grows } else { shrinks }, amount)))
}

/// The pane a forwarded key belongs in: the one spanning the whole content column of `own`'s
/// tab, read the way `adjust_plan` reads it; the active one when a vertical split stacks several.
pub fn content_pane(panes: &[PaneInfo], own: u64) -> Result<u64, String> {
    let me = panes
        .iter()
        .find(|p| p.pane_id == own)
        .ok_or_else(|| format!("own pane {own} not listed"))?;
    let tab: Vec<&PaneInfo> = panes.iter().filter(|p| p.tab_id == me.tab_id).collect();
    let tab_cols = tab.iter().map(|p| p.left_col + p.cols).max().unwrap_or(0);
    let content_width = tab_cols - me.cols - 1;
    let spanning: Vec<&PaneInfo> = tab
        .iter()
        .copied()
        .filter(|p| p.pane_id != own && p.cols == content_width && !is_marker(&p.title))
        .collect();
    let first = spanning.first().ok_or("no pane spans the content column")?;
    Ok(spanning
        .iter()
        .find(|p| p.is_active)
        .unwrap_or(first)
        .pane_id)
}

pub struct Cli {
    exe: PathBuf,
    own_pane: u64,
}

/// Whether `pane` is a backend pane listed on this server, the only thing a kill by id may hit.
pub fn kill_target(panes: &[PaneInfo], pane: u64) -> Result<(), String> {
    let target = panes
        .iter()
        .find(|p| p.pane_id == pane)
        .ok_or_else(|| format!("pane {pane} not on this server"))?;
    if is_marker(&target.title) {
        Ok(())
    } else {
        Err(format!("pane {pane} is not a backend"))
    }
}

/// A backend's own title marker, in either role.
pub fn is_marker(title: &str) -> bool {
    let nonce = title
        .strip_prefix("wez-vtabs:")
        .or_else(|| title.strip_prefix("wez-vtabs-settings:"));
    nonce.is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// The panes of `own`'s tab inside the sidebar's band, and the content pane to put them under.
/// Mirrors `sidebar_rescue.intruders`: the band is the width the sidebar is meant to have.
pub fn rescue_plan(
    panes: &[PaneInfo],
    own: u64,
    band: i64,
    right: bool,
) -> Result<(Vec<u64>, u64), String> {
    let me = panes
        .iter()
        .find(|p| p.pane_id == own)
        .ok_or_else(|| format!("own pane {own} not listed"))?;
    let tab: Vec<&PaneInfo> = panes.iter().filter(|p| p.tab_id == me.tab_id).collect();
    let tab_cols = tab.iter().map(|p| p.left_col + p.cols).max().unwrap_or(0);
    let inside = |p: &PaneInfo| {
        if right {
            p.left_col + p.cols >= tab_cols - band
        } else {
            p.left_col <= band
        }
    };
    let intruders: Vec<u64> = tab
        .iter()
        .filter(|p| p.pane_id != own && inside(p))
        .map(|p| p.pane_id)
        .collect();
    if intruders.is_empty() {
        return Err("nothing inside the band".into());
    }
    let host = tab
        .iter()
        .find(|p| p.pane_id != own && !inside(p) && !is_marker(&p.title))
        .map(|p| p.pane_id)
        .ok_or("no content pane to move under")?;
    Ok((intruders, host))
}

impl Cli {
    /// A cli for `own_pane` at `exe`; `from_env` is the production path.
    pub fn at(exe: PathBuf, own_pane: u64) -> Self {
        Cli { exe, own_pane }
    }

    pub fn own_pane(&self) -> u64 {
        self.own_pane
    }

    /// None where WezTerm set no pane id: nothing to act on behalf of.
    pub fn from_env() -> Option<Self> {
        let own_pane = std::env::var("WEZTERM_PANE").ok()?.trim().parse().ok()?;
        let beside = std::env::var_os("WEZTERM_EXECUTABLE_DIR")
            .map(|dir| PathBuf::from(dir).join(BIN))
            .filter(|path| path.is_file());
        Some(Cli {
            exe: beside.unwrap_or_else(|| PathBuf::from(BIN)),
            own_pane,
        })
    }

    /// Stdout of `wezterm cli --no-auto-start <args>`, or the first line of what went wrong.
    fn run(&self, args: &[&str]) -> Result<String, String> {
        self.run_with(args, None)
    }

    fn run_with(&self, args: &[&str], input: Option<&[u8]>) -> Result<String, String> {
        let mut command = Command::new(&self.exe);
        command.arg("cli").arg("--no-auto-start");
        // A pane hosted by a standalone or SSH mux inherits that server's socket.  Without this
        // flag the CLI treats it as a GUI socket and waits on the wrong protocol endpoint.
        if is_mux_socket(std::env::var_os("WEZTERM_UNIX_SOCKET").as_deref()) {
            command.arg("--prefer-mux");
        }
        command.args(args);
        let out = run_command(
            &mut command,
            TIMEOUT,
            &format!("wezterm cli {}", args[0]),
            input,
        )?;
        if out.status.success() {
            return Ok(String::from_utf8_lossy(&out.stdout).into_owned());
        }
        let err = String::from_utf8_lossy(&out.stderr);
        Err(err
            .lines()
            .next()
            .unwrap_or("wezterm cli failed")
            .to_string())
    }

    fn list(&self) -> Result<Vec<PaneInfo>, String> {
        panes_from_json(&self.run(&["list", "--format", "json"])?)
    }

    /// Kills `pane` once this server lists it as a backend of this plugin: an id that crossed
    /// from another server would otherwise name whatever pane shares the number here.
    pub fn kill_pane(&self, pane: u64) -> Result<(), String> {
        kill_target(&self.list()?, pane)?;
        self.run(&["kill-pane", "--pane-id", &pane.to_string()])
            .map(|_| ())
    }

    /// Writes `bytes` into `pane` as typed input, not a paste. They travel on stdin, so no byte
    /// can be read as a flag.
    pub fn send_text(&self, pane: u64, bytes: &[u8]) -> Result<(), String> {
        self.run_with(
            &["send-text", "--no-paste", "--pane-id", &pane.to_string()],
            Some(bytes),
        )
        .map(|_| ())
    }

    /// Delivers a forwarded key to the pane spanning this tab's content column; that pane's id.
    pub fn forward_key(&self, bytes: &[u8]) -> Result<u64, String> {
        let pane = content_pane(&self.list()?, self.own_pane)?;
        self.send_text(pane, bytes)?;
        Ok(pane)
    }

    /// Resizes this pane's own split to `target`, preserving the content pane's focus.
    pub fn adjust(&self, target: u32, min_content: u32) -> Result<(), String> {
        let own = self.own_pane.to_string();
        let panes = self.list()?;
        let Some((direction, amount)) = adjust_delta(&panes, self.own_pane, target, min_content)?
        else {
            return Ok(());
        };
        let displaced = adjust_plan(&panes, self.own_pane)?;
        if displaced.is_some() {
            self.run(&["activate-pane", "--pane-id", &own])?;
        }
        let adjusted = self
            .run(&[
                "adjust-pane-size",
                "--pane-id",
                &own,
                "--amount",
                &amount.to_string(),
                direction,
            ])
            .map(|_| ());
        let restored = displaced.map_or(Ok(()), |pane| {
            self.run(&["activate-pane", "--pane-id", &pane.to_string()])
                .map(|_| ())
        });
        match (adjusted, restored) {
            (Err(err), _) => Err(err),
            (Ok(()), Err(err)) => Err(err),
            (Ok(()), Ok(())) => Ok(()),
        }
    }

    /// Moves every intruder under the host; the count moved, or the first failure.
    pub fn rescue(&self, band: i64, right: bool) -> Result<usize, String> {
        let (intruders, host) = rescue_plan(&self.list()?, self.own_pane, band, right)?;
        let host = host.to_string();
        for id in &intruders {
            self.run(&[
                "split-pane",
                "--move-pane-id",
                &id.to_string(),
                "--pane-id",
                &host,
                "--bottom",
            ])?;
        }
        Ok(intruders.len())
    }
}

#[cfg(test)]
mod parser_tests {
    use super::*;

    #[test]
    fn kill_reaches_only_a_backend_listed_on_this_server() {
        let pane = |pane_id, title: &str| PaneInfo {
            pane_id,
            tab_id: 7,
            title: title.into(),
            left_col: 0,
            cols: 30,
            is_active: false,
        };
        let panes = vec![
            pane(3, "wez-vtabs:beef"),
            pane(4, "zsh"),
            pane(5, "wez-vtabs-settings:cafe"),
        ];
        assert_eq!(kill_target(&panes, 3), Ok(()));
        assert_eq!(kill_target(&panes, 5), Ok(()));
        assert_eq!(
            kill_target(&panes, 4),
            Err("pane 4 is not a backend".into())
        );
        assert_eq!(
            kill_target(&panes, 9),
            Err("pane 9 not on this server".into())
        );
    }

    #[test]
    fn standalone_socket_prefers_the_mux_protocol() {
        assert!(is_mux_socket(Some(OsStr::new("/tmp/e2e/mux.sock"))));
        assert!(is_mux_socket(Some(OsStr::new(
            "/run/user/1000/wezterm-mux"
        ))));
    }

    #[test]
    fn gui_socket_keeps_the_gui_protocol() {
        assert!(!is_mux_socket(Some(OsStr::new(
            "/run/user/1000/wezterm/gui-sock-42"
        ))));
        assert!(!is_mux_socket(None));
    }

    #[test]
    fn negative_pane_and_tab_ids_are_skipped_before_conversion() {
        let json = r#"[
            {"pane_id": 3, "tab_id": 7, "title": "valid", "left_col": 0, "size": {"cols": 80}},
            {"pane_id": -1, "tab_id": 7, "title": "pane", "left_col": 0, "size": {"cols": 80}},
            {"pane_id": 4, "tab_id": -1, "title": "tab", "left_col": 0, "size": {"cols": 80}}
        ]"#;
        assert_eq!(
            panes_from_json(json).unwrap(),
            vec![PaneInfo {
                pane_id: 3,
                tab_id: 7,
                title: "valid".into(),
                left_col: 0,
                cols: 80,
                is_active: false,
            }]
        );
    }

    #[test]
    fn invalid_geometry_is_skipped() {
        let json = format!(
            r#"[
                {{"pane_id": 1, "tab_id": 7, "title": "negative position", "left_col": -1, "size": {{"cols": 80}}}},
                {{"pane_id": 2, "tab_id": 7, "title": "negative size", "left_col": 0, "size": {{"cols": -1}}}},
                {{"pane_id": 3, "tab_id": 7, "title": "zero size", "left_col": 0, "size": {{"cols": 0}}}},
                {{"pane_id": 4, "tab_id": 7, "title": "overflow", "left_col": {}, "size": {{"cols": 1}}}},
                {{"pane_id": 5, "tab_id": 7, "title": "wrong type", "left_col": "0", "size": {{"cols": 80}}}}
            ]"#,
            i64::MAX
        );
        assert!(panes_from_json(&json).unwrap().is_empty());
    }
}

#[cfg(all(test, unix))]
mod command_tests {
    use super::*;

    #[test]
    fn command_output_is_drained_before_the_child_exits() {
        let mut command = Command::new("sh");
        command.args([
            "-c",
            "dd if=/dev/zero bs=65536 count=4 2>/dev/null; dd if=/dev/zero bs=65536 count=4 1>&2 2>/dev/null",
        ]);
        let out = run_command(&mut command, Duration::from_secs(3), "bulk output", None).unwrap();
        assert!(out.status.success());
        assert_eq!(out.stdout.len(), 4 * 65_536, "larger than a pipe buffer");
        assert_eq!(out.stderr.len(), 4 * 65_536, "both pipes are drained");
    }

    #[test]
    fn command_output_capture_is_bounded_while_both_pipes_are_drained() {
        let mut command = Command::new("sh");
        command.args([
            "-c",
            "dd if=/dev/zero bs=65536 count=17 2>/dev/null; dd if=/dev/zero bs=65536 count=17 1>&2 2>/dev/null",
        ]);
        assert_eq!(
            run_command(&mut command, Duration::from_secs(3), "excess output", None).unwrap_err(),
            format!("excess output: output exceeded {OUTPUT_MAX} bytes")
        );
    }

    #[test]
    fn a_timed_out_command_is_killed_and_reaped() {
        let mut command = Command::new("sh");
        // The one-second descendant inherits both pipes after the five-second direct child dies.
        command.args(["-c", "sleep 1 & exec sleep 5"]);
        let started = Instant::now();
        assert_eq!(
            run_command(
                &mut command,
                Duration::from_millis(30),
                "slow command",
                None
            )
            .unwrap_err(),
            "slow command timed out"
        );
        assert!(started.elapsed() < Duration::from_millis(750));
    }
}
