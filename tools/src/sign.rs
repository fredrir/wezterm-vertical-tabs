//! MacOS code signing with a rebuild-stable identity when a Developer ID is in the keychain.

use std::fs;
use std::path::Path;

use anyhow::{Result, bail};
use serde_json::json;

use crate::process::CommandSpec;
use crate::state::Context;

const DEVELOPER_ID: &str = "Developer ID Application: ";
const BUNDLE_IDENTIFIER: &str = "com.github.wez.wezterm";
const MAIN_EXECUTABLE: &str = "wezterm-gui";
const AD_HOC: &str = "-";

struct Identity {
    hash: String,
    name: String,
}

pub fn sign_app(ctx: &Context, app: &Path) -> Result<()> {
    let listing = ctx.runner.capture(
        CommandSpec::new("security")
            .args(["find-identity", "-v", "-p", "codesigning"])
            .cwd(&ctx.root),
    )?;
    let identity = developer_id(&listing)?;
    let signer = match &identity {
        Some(identity) => identity.hash.as_str(),
        None => {
            eprintln!("sign: no Developer ID identity in the keychain; signing ad-hoc");
            AD_HOC
        }
    };
    ctx.runner.metadata(
        "signing_identity",
        &json!(
            identity
                .as_ref()
                .map_or("ad-hoc", |identity| identity.name.as_str())
        ),
    )?;

    let mut helpers = fs::read_dir(app.join("Contents/MacOS"))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()?;
    helpers.sort();
    for helper in helpers
        .iter()
        .filter(|path| path.file_name() != Some(MAIN_EXECUTABLE.as_ref()))
    {
        let name = helper.file_name().unwrap_or_default().to_string_lossy();
        ctx.runner.run(
            codesign(ctx, signer)
                .arg("--identifier")
                .arg(format!("{BUNDLE_IDENTIFIER}.{name}"))
                .arg(helper),
        )?;
    }
    ctx.runner.run(codesign(ctx, signer).arg(app))?;
    ctx.runner.run(
        CommandSpec::new("codesign")
            .args(["--verify", "--deep", "--strict"])
            .arg(app)
            .cwd(&ctx.root),
    )
}

fn codesign(ctx: &Context, signer: &str) -> CommandSpec {
    CommandSpec::new("codesign")
        .args(["--force", "--timestamp=none", "--sign", signer])
        .cwd(&ctx.root)
}

fn developer_id(listing: &str) -> Result<Option<Identity>> {
    let identities: Vec<Identity> = listing
        .lines()
        .filter_map(parse_identity)
        .filter(|identity| identity.name.starts_with(DEVELOPER_ID))
        .collect();
    let mut names: Vec<&str> = identities
        .iter()
        .map(|identity| identity.name.as_str())
        .collect();
    names.sort_unstable();
    names.dedup();
    if names.len() > 1 {
        bail!(
            "multiple Developer ID identities in the keychain: {}; keep only one",
            names.join(", ")
        );
    }
    Ok(identities.into_iter().next())
}

fn parse_identity(line: &str) -> Option<Identity> {
    let (_, rest) = line.trim().split_once(") ")?;
    let (hash, quoted) = rest.split_once(' ')?;
    let name = quoted.strip_prefix('"')?.strip_suffix('"')?;
    Some(Identity {
        hash: hash.into(),
        name: name.into(),
    })
}
