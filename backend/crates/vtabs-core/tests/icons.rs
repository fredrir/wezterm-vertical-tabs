use std::collections::BTreeMap;

use vtabs_core::icons::{IconSet, for_process, resolve};

fn set(pairs: &[(&str, &str)]) -> IconSet {
    resolve(
        &pairs
            .iter()
            .map(|&(k, v)| (k.to_string(), v.to_string()))
            .collect::<BTreeMap<_, _>>(),
    )
}

#[test]
fn overrides_and_patterns() {
    let icons = set(&[("nvim", "N"), ("^cargo%-", "R")]);
    assert_eq!(for_process("/usr/bin/nvim", &icons), "N");
    assert_eq!(for_process("cargo-watch", &icons), "R");
    assert_ne!(
        for_process("cargo", &icons),
        "R",
        "the pattern needs its dash"
    );
    assert_eq!(for_process("unknown-thing", &icons), icons.map["default"]);
    assert_eq!(for_process("", &icons), icons.map["default"]);
}

#[test]
fn process_key_strips_path_and_login_dash() {
    let icons = set(&[("zsh", "Z"), ("htop", "H"), ("pwsh.exe", "P")]);
    assert_eq!(for_process("-zsh", &icons), "Z");
    assert_eq!(for_process("/usr/local/bin/htop", &icons), "H");
    assert_eq!(
        for_process("C:\\Program Files\\PowerShell\\pwsh.exe", &icons),
        "P"
    );
}
