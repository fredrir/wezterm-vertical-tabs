use ratatui::style::Color;
use std::collections::BTreeMap;

pub(crate) const SIDEBAR: &str = "\u{f10aa}";
pub(crate) const SETTINGS: &str = "\u{f0493}";
pub(crate) const REFRESH: &str = "\u{f0450}";
pub(crate) const SEARCH: &str = "\u{f0349}";
pub(crate) const PLUS: &str = "\u{f0415}";
pub(crate) const CLOSE: &str = "\u{f0156}";
pub(crate) const SPACE: &str = "\u{f0328}";
pub(crate) const EXPANDED: &str = "\u{f0140}";
pub(crate) const COLLAPSED: &str = "\u{f0142}";
pub(crate) const FOLDER: &str = "\u{f0256}";
pub(crate) const FOLDER_OPEN: &str = "\u{f0dcf}";
pub(crate) const LOCAL: &str = "\u{f120}";
pub(crate) const REMOTE: &str = "\u{f048b}";
pub(crate) const COMMAND: &str = "\u{f0633}";
pub(crate) const ALERT: &str = "\u{f05d6}";
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

const MACHINES: &[(&str, &str, Color)] = &[
    ("arch", "\u{f303}", Color::Rgb(0xe8, 0x64, 0x64)),
    ("ubuntu", "\u{ef72}", Color::Rgb(0xff, 0xa0, 0x5e)),
    ("debian", "\u{f306}", Color::Rgb(0xff, 0x7e, 0xa6)),
    ("fedora", "\u{f30a}", Color::Rgb(0xc9, 0xa4, 0xff)),
    ("nixos", "\u{f313}", Color::Rgb(0xc3, 0xee, 0x7a)),
    ("alpine", "\u{f300}", Color::Rgb(0xee, 0xe5, 0x7a)),
    ("centos", "\u{f304}", Color::Rgb(0xe9, 0xa0, 0xff)),
    ("rhel", "\u{f316}", Color::Rgb(0xff, 0x76, 0x76)),
    ("opensuse", "\u{f314}", Color::Rgb(0x8b, 0xf0, 0x8a)),
    ("manjaro", "\u{f312}", Color::Rgb(0x5e, 0xe8, 0xa4)),
    ("raspbian", "\u{f315}", Color::Rgb(0xff, 0x9a, 0xd5)),
    ("gentoo", "\u{f30d}", Color::Rgb(0xe0, 0xc8, 0xff)),
    ("freebsd", "\u{f30c}", Color::Rgb(0xff, 0xb3, 0xa6)),
    ("linux", "\u{f31a}", Color::Rgb(0xff, 0xcb, 0x6b)),
    ("macos", "\u{f179}", Color::Rgb(0xe4, 0xe4, 0xea)),
    ("windows", "\u{f17a}", Color::Rgb(0xc6, 0xf7, 0xe2)),
];
pub(crate) const UNKNOWN_REMOTE: &str = "remote";
const UNKNOWN_REMOTE_COLOR: Color = Color::Rgb(0xff, 0xe9, 0xa8);

fn machine(os: &str) -> Option<&'static (&'static str, &'static str, Color)> {
    MACHINES.iter().find(|(id, ..)| *id == os)
}

pub(crate) fn host(remote: bool, os: &str) -> &'static str {
    match machine(os) {
        _ if !remote => LOCAL,
        Some((_, glyph, _)) => glyph,
        None => REMOTE,
    }
}

pub(crate) fn host_color(
    remote: bool,
    os: &str,
    overrides: &BTreeMap<String, Color>,
) -> Option<Color> {
    if !remote {
        return None;
    }
    let (key, color) = machine(os)
        .map_or((UNKNOWN_REMOTE, UNKNOWN_REMOTE_COLOR), |(id, _, color)| {
            (*id, *color)
        });
    Some(overrides.get(key).copied().unwrap_or(color))
}
