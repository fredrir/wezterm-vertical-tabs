//! `wezterm cli` from inside the pane. This process runs where the pane lives, with the server's
//! socket and its own pane id in the environment, so it can kill and move panes there that the
//! GUI's mux client cannot reach. Every call passes `--no-auto-start`: a cli that finds no server
//! must never spawn one over the socket path.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

const TIMEOUT: Duration = Duration::from_secs(5);
#[cfg(windows)]
const BIN: &str = "wezterm.exe";
#[cfg(not(windows))]
const BIN: &str = "wezterm";

/// The fields of `wezterm cli list --format json` the two verbs read.
#[derive(Debug, Clone, PartialEq)]
pub struct PaneInfo {
    pub pane_id: u64,
    pub tab_id: u64,
    pub title: String,
    pub left_col: i64,
    pub cols: i64,
}

/// `cli list --format json`, one entry per pane; an entry missing an id is skipped.
pub fn panes_from_json(json: &str) -> Result<Vec<PaneInfo>, String> {
    let list: Vec<Value> = serde_json::from_str(json).map_err(|e| format!("cli list: {e}"))?;
    let int = |v: &Value, key: &str| v.get(key).and_then(Value::as_i64);
    Ok(list
        .iter()
        .filter_map(|v| {
            Some(PaneInfo {
                pane_id: int(v, "pane_id")? as u64,
                tab_id: int(v, "tab_id")? as u64,
                title: v.get("title")?.as_str()?.to_string(),
                left_col: int(v, "left_col").unwrap_or(0),
                cols: v.get("size").and_then(|s| int(s, "cols")).unwrap_or(0),
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
        let mut child = Command::new(&self.exe)
            .arg("cli")
            .arg("--no-auto-start")
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("{}: {e}", self.exe.display()))?;
        let started = Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if started.elapsed() < TIMEOUT => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                Ok(None) => {
                    let _ = child.kill();
                    return Err(format!("wezterm cli {} timed out", args[0]));
                }
                Err(e) => return Err(e.to_string()),
            }
        }
        let out = child.wait_with_output().map_err(|e| e.to_string())?;
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
mod tests {
    use super::*;

    fn pane(id: u64, tab: u64, title: &str, left: i64, cols: i64) -> PaneInfo {
        PaneInfo {
            pane_id: id,
            tab_id: tab,
            title: title.into(),
            left_col: left,
            cols,
        }
    }

    #[test]
    fn only_a_backend_title_names_a_kill_and_only_one_pane_may_carry_it() {
        let panes = vec![
            pane(1, 1, "wez-vtabs:abcd", 0, 28),
            pane(2, 1, "zsh", 29, 100),
            pane(3, 2, "wez-vtabs:ffff", 0, 28),
            pane(4, 2, "wez-vtabs:ffff", 29, 28),
        ];
        assert_eq!(kill_target(&panes, "wez-vtabs:abcd"), Ok(1));
        assert!(
            kill_target(&panes, "zsh").is_err(),
            "a shell is never a target"
        );
        assert!(kill_target(&panes, "wez-vtabs:0000").is_err(), "unknown");
        assert!(kill_target(&panes, "wez-vtabs:ffff").is_err(), "ambiguous");
        assert!(is_marker("wez-vtabs-settings:1a2b"));
        assert!(!is_marker("wez-vtabs:") && !is_marker("wez-vtabs:zz"));
    }

    #[test]
    fn the_plan_moves_the_pane_inside_the_band_under_the_content_beside_it() {
        // a SplitHorizontal on the sidebar: the shell landed at column 15, inside the 28 the
        // sidebar is meant to have, and the sidebar's own box shrank to 14
        let panes = vec![
            pane(1, 7, "wez-vtabs:abcd", 0, 14),
            pane(2, 7, "zsh", 15, 13),
            pane(3, 7, "nvim", 29, 100),
            pane(9, 8, "zsh", 0, 129),
        ];
        assert_eq!(rescue_plan(&panes, 1, 28, false), Ok((vec![2], 3)));
        assert!(
            rescue_plan(&panes, 9, 28, false).is_err(),
            "another tab has nothing inside its band"
        );
        let right = vec![
            pane(1, 7, "nvim", 0, 100),
            pane(2, 7, "zsh", 101, 13),
            pane(3, 7, "wez-vtabs:abcd", 115, 14),
        ];
        assert_eq!(rescue_plan(&right, 3, 28, true), Ok((vec![2], 1)));
    }

    #[test]
    fn the_list_is_read_from_the_cli_json_shape() {
        let json = r#"[{"window_id":0,"tab_id":7,"pane_id":3,"title":"zsh","left_col":29,"top_row":0,
            "size":{"rows":60,"cols":100,"pixel_width":1,"pixel_height":1,"dpi":144},"is_active":true},
            {"tab_id":7,"pane_id":"x"}]"#;
        assert_eq!(
            panes_from_json(json).unwrap(),
            vec![pane(3, 7, "zsh", 29, 100)],
            "a malformed entry is skipped, not fatal"
        );
        assert!(panes_from_json("nope").is_err());
    }

    #[test]
    fn a_tab_with_no_content_outside_the_band_has_nowhere_to_move_to() {
        let panes = vec![
            pane(1, 7, "wez-vtabs:abcd", 0, 14),
            pane(2, 7, "zsh", 15, 13),
        ];
        assert!(rescue_plan(&panes, 1, 28, false).is_err());
    }
}
