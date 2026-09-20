//! Nerd Font glyphs. A trailing blank cell lets the renderer draw them at natural size.
use vtabs_core::Tab;

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

/// The local machine always reads as a terminal; remotes read as their operating system.
pub(crate) fn host(tab: &Tab) -> &'static str {
    if !tab.remote {
        return LOCAL;
    }
    match tab.os.as_str() {
        "arch" => "\u{f08c7}",
        "ubuntu" => "\u{ef72}",
        "debian" => "\u{f306}",
        "fedora" => "\u{f30a}",
        "nixos" => "\u{f313}",
        "alpine" => "\u{f300}",
        "centos" => "\u{f304}",
        "rhel" => "\u{f316}",
        "opensuse" => "\u{f314}",
        "manjaro" => "\u{f312}",
        "raspbian" => "\u{f315}",
        "gentoo" => "\u{f30d}",
        "freebsd" => "\u{f30c}",
        "linux" => "\u{f31a}",
        "macos" => "\u{f179}",
        "windows" => "\u{f17a}",
        _ => REMOTE,
    }
}
