use ratatui::style::Color;
use std::collections::BTreeMap;

pub(crate) const SIDEBAR: &str = "\u{f10aa}";
pub(crate) const SETTINGS: &str = "\u{f0493}";
pub(crate) const REFRESH: &str = "\u{f0450}";
pub(crate) const SEARCH: &str = "\u{f0349}";
pub(crate) const KEYBOARD: &str = "\u{f030c}";
pub(crate) const PLUS: &str = "\u{f0415}";
pub(crate) const CLOSE: &str = "\u{f0156}";
pub(crate) const SPACE: &str = "\u{f0328}";
pub(crate) const EXPANDED: &str = "\u{f0140}";
pub(crate) const COLLAPSED: &str = "\u{f0142}";
pub(crate) const FOLDER: &str = "\u{f0256}";
pub(crate) const FOLDER_OPEN: &str = "\u{f0dcf}";
pub(crate) const LOCAL: &str = "\u{f120}";
pub(crate) const REMOTE: &str = "\u{f048b}";
pub(crate) const ALERT: &str = "\u{f05d6}";
pub(crate) const LOCK: &str = "\u{f0341}";
pub(crate) const CHECK: &str = "\u{f012c}";
pub(crate) const RESTORE: &str = "\u{f099b}";
pub(crate) const ALL: &str = "\u{f11d9}";
pub(crate) const LAYOUT: &str = "\u{f06fd}";
pub(crate) const TERMINAL: &str = "\u{f018d}";
pub(crate) const TAB: &str = "\u{f04e9}";
pub(crate) const PALETTE: &str = "\u{f0e0c}";
pub(crate) const MOTION: &str = "\u{f0a8f}";
pub(crate) const TUNE: &str = "\u{f1542}";
pub(crate) const KEYBOARD_OUTLINE: &str = "\u{f097b}";
const SPACE_DOT: &str = "\u{f0ec3}";

pub(crate) fn index(index: Option<usize>) -> Option<&'static str> {
    const DIGITS: [&str; 9] = [
        "\u{f03a6}",
        "\u{f03a9}",
        "\u{f03ac}",
        "\u{f03ae}",
        "\u{f03b0}",
        "\u{f03b5}",
        "\u{f03b8}",
        "\u{f03bb}",
        "\u{f03be}",
    ];
    DIGITS.get(index?.checked_sub(1)?).copied()
}

pub(crate) fn space(icon: &str) -> &str {
    if icon == "◉" { SPACE_DOT } else { icon }
}

const MACHINES: &[(&str, &str)] = &[
    ("arch", "\u{f303}"),
    ("ubuntu", "\u{ef72}"),
    ("debian", "\u{f306}"),
    ("fedora", "\u{f30a}"),
    ("nixos", "\u{f313}"),
    ("alpine", "\u{f300}"),
    ("centos", "\u{f304}"),
    ("rhel", "\u{f316}"),
    ("opensuse", "\u{f314}"),
    ("manjaro", "\u{f312}"),
    ("raspbian", "\u{f315}"),
    ("gentoo", "\u{f30d}"),
    ("freebsd", "\u{f30c}"),
    ("linux", "\u{f31a}"),
    ("macos", "\u{f179}"),
    ("windows", "\u{f17a}"),
];
const UNKNOWN_REMOTE: &str = "remote";

fn machine(os: &str) -> Option<&'static (&'static str, &'static str)> {
    MACHINES.iter().find(|(id, _)| *id == os)
}

pub(crate) fn host(remote: bool, os: &str) -> &'static str {
    match machine(os) {
        _ if !remote => LOCAL,
        Some((_, glyph)) => glyph,
        None => REMOTE,
    }
}

/// `colors` holds `distro_colors`, where `remote` covers unrecognized machines.
pub(crate) fn host_color(
    remote: bool,
    os: &str,
    colors: &BTreeMap<String, Color>,
) -> Option<Color> {
    if !remote {
        return None;
    }
    colors
        .get(machine(os).map_or(UNKNOWN_REMOTE, |(id, _)| *id))
        .copied()
}
