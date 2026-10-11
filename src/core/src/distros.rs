use crate::settings::{SettingDescriptor, SettingKind, valid_color};
use serde::{Deserialize, Deserializer};
use serde_json::Value;
use std::collections::BTreeMap;

macro_rules! distros {
    ($(($id:literal, $label:literal, $color:literal)),* $(,)?) => {
        /// Remote glyph rows, each editing one entry of `distro_colors`.
        pub const DESCRIPTORS: &[SettingDescriptor] = &[
            $(SettingDescriptor {
                key: concat!("distro.", $id),
                label: $label,
                group: "theme",
                kind: SettingKind::Color,
                description: "Remote glyph color",
            }),*
        ];

        const DEFAULTS: &[(&str, &str)] = &[$(($id, $color)),*];
    };
}

distros! {
    ("arch", "Arch", "#e86464"),
    ("ubuntu", "Ubuntu", "#ffa05e"),
    ("debian", "Debian", "#ff7ea6"),
    ("fedora", "Fedora", "#c9a4ff"),
    ("nixos", "NixOS", "#c3ee7a"),
    ("alpine", "Alpine", "#eee57a"),
    ("centos", "CentOS", "#e9a0ff"),
    ("rhel", "RHEL", "#ff7676"),
    ("opensuse", "openSUSE", "#8bf08a"),
    ("manjaro", "Manjaro", "#5ee8a4"),
    ("raspbian", "Raspbian", "#ff9ad5"),
    ("gentoo", "Gentoo", "#e0c8ff"),
    ("freebsd", "FreeBSD", "#ffb3a6"),
    ("linux", "Linux", "#ffcb6b"),
    ("macos", "macOS", "#e4e4ea"),
    ("windows", "Windows", "#c6f7e2"),
    ("remote", "Unknown", "#ffe9a8"),
}

pub fn ids() -> impl Iterator<Item = &'static str> {
    DEFAULTS.iter().map(|(id, _)| *id)
}

pub fn defaults() -> BTreeMap<String, String> {
    DEFAULTS
        .iter()
        .map(|(id, color)| ((*id).into(), (*color).into()))
        .collect()
}

/// The OS ID a remote glyph row edits.
pub fn id(key: &str) -> Option<&str> {
    key.strip_prefix("distro.")
}

pub fn validate(value: &Value) -> Result<(), String> {
    let colors = value
        .as_object()
        .ok_or_else(|| "Invalid value for distro_colors".to_owned())?;
    for (id, color) in colors {
        if !ids().any(|known| known == id) {
            return Err(format!("Unknown OS ID: {id}"));
        }
        if !color.as_str().is_some_and(valid_color) {
            return Err(format!("Invalid color for {id}"));
        }
    }
    Ok(())
}

/// Entries left out keep their default color.
pub(crate) fn deserialize<'de, D>(deserializer: D) -> Result<BTreeMap<String, String>, D::Error>
where
    D: Deserializer<'de>,
{
    let mut colors = defaults();
    colors.extend(BTreeMap::<String, String>::deserialize(deserializer)?);
    Ok(colors)
}
