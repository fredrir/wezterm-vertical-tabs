//! `wezterm cli` from inside the pane. This process runs where the pane lives, with the server's
//! socket and its own pane id in the environment, so it can kill and move panes there that the
//! GUI's mux client cannot reach. Every call passes `--no-auto-start`: a cli that finds no server
//! must never spawn one over the socket path.

use std::ffi::OsStr;
use std::io::{self, Read};
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
/// descendant.
fn run_command(command: &mut Command, timeout: Duration, label: &str) -> Result<Output, String> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("{label}: {e}"))?;
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

/// The fields of `wezterm cli list --format json` the two verbs read.
#[derive(Debug, Clone, PartialEq)]
pub struct PaneInfo {
    pub pane_id: u64,
    pub tab_id: u64,
    pub title: String,
    pub left_col: i64,
    pub cols: i64,
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
            })
        })
        .collect())
}

pub struct Cli {
    exe: PathBuf,
    own_pane: u64,
}

/// A backend's own title marker, in either role; the one kind of title a `kill` may name.
pub fn is_marker(title: &str) -> bool {
    let nonce = title
        .strip_prefix("wez-vtabs:")
        .or_else(|| title.strip_prefix("wez-vtabs-settings:"));
    nonce.is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// The pane to kill for `title`: exactly one match, or the reason there is none.
pub fn kill_target(panes: &[PaneInfo], title: &str) -> Result<u64, String> {
    if !is_marker(title) {
        return Err(format!("not a backend title: {title}"));
    }
    let mut hits = panes.iter().filter(|p| p.title == title);
    match (hits.next(), hits.next()) {
        (Some(p), None) => Ok(p.pane_id),
        (None, _) => Err(format!("no pane titled {title}")),
        (Some(_), Some(_)) => Err(format!("more than one pane titled {title}")),
    }
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
        let mut command = Command::new(&self.exe);
        command.arg("cli").arg("--no-auto-start");
        // A pane hosted by a standalone or SSH mux inherits that server's socket.  Without this
        // flag the CLI treats it as a GUI socket and waits on the wrong protocol endpoint.
        if is_mux_socket(std::env::var_os("WEZTERM_UNIX_SOCKET").as_deref()) {
            command.arg("--prefer-mux");
        }
        command.args(args);
        let out = run_command(&mut command, TIMEOUT, &format!("wezterm cli {}", args[0]))?;
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

    pub fn kill_by_title(&self, title: &str) -> Result<(), String> {
        let id = kill_target(&self.list()?, title)?;
        self.run(&["kill-pane", "--pane-id", &id.to_string()])
            .map(|_| ())
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
        let out = run_command(&mut command, Duration::from_secs(3), "bulk output").unwrap();
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
            run_command(&mut command, Duration::from_secs(3), "excess output").unwrap_err(),
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
            run_command(&mut command, Duration::from_millis(30), "slow command").unwrap_err(),
            "slow command timed out"
        );
        assert!(started.elapsed() < Duration::from_millis(750));
    }
}
