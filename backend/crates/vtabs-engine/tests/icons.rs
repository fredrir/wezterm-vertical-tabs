use std::collections::BTreeMap;

use vtabs_engine::icons::{IconSet, for_process, resolve};

fn set(pairs: &[(&str, &str)]) -> IconSet {
    resolve(
        &pairs
            .iter()
            .map(|&(k, v)| (k.to_string(), v.to_string()))
            .collect::<BTreeMap<_, _>>(),
    )
}

#[test]
fn process_icons_are_exact_builtins_not_user_pattern_emulation() {
    let icons = set(&[("nvim", "N"), ("^cargo%-", "R")]);
    assert_ne!(for_process("/usr/bin/nvim", &icons), "N");
    assert_ne!(for_process("cargo-watch", &icons), "R");
    assert_eq!(for_process("unknown-thing", &icons), icons.map["default"]);
    assert_eq!(for_process("", &icons), icons.map["default"]);
}

#[test]
fn process_key_strips_path_and_login_dash() {
    let icons = set(&[("zsh", "Z"), ("htop", "H"), ("pwsh.exe", "P")]);
    assert_ne!(for_process("-zsh", &icons), "Z");
    assert_ne!(for_process("/usr/local/bin/htop", &icons), "H");
    assert_ne!(
        for_process("C:\\Program Files\\PowerShell\\pwsh.exe", &icons),
        "P"
    );
}
