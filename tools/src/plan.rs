use anyhow::Result;
use serde_json::{Value, json};

use crate::source::inventory;
use crate::state::Context;

pub fn plan(ctx: &Context, operation: &str) -> Result<Value> {
    let previous = ctx.last_build()?;
    let inputs = json!({"source":inventory::source_digest(&ctx.root)?, "compile":inventory::compile_digest(&ctx.root)?, "validation":inventory::validation_digest(&ctx.root)?});
    let unchanged = previous
        .as_ref()
        .is_some_and(|old| inputs["source"].as_str() == Some(&old.source_digest));
    let compile_changed = previous.as_ref().is_none_or(|old| {
        Some(old.configuration.compilation.compile_inputs.as_str()) != inputs["compile"].as_str()
    });
    let validation_changed = previous.as_ref().is_none_or(|old| {
        Some(old.configuration.validation_inputs.as_str()) != inputs["validation"].as_str()
    });
    let upstream = if operation == "deploy" {
        crate::remote::pinned_upstream(ctx)?
    } else {
        ctx.upstream.clone().or_else(|| {
            (ctx.offline || operation == "dev")
                .then(|| previous.as_ref().map(|old| old.upstream.clone()))
                .flatten()
        })
    };
    let mut steps = vec![
        json!({"name":"resolve","action":if upstream.is_some(){"check"}else{"run"},"reason":if upstream.is_some(){"use selected upstream revision"}else{"resolve latest main once; plan does not fetch"}}),
    ];
    if operation == "check" {
        steps.clear();
        steps.push(json!({"name":"validate","action":"run","reason":"run project format, lint, schema and behavioral checks"}));
    } else {
        steps.push(json!({"name":"prepare","action":"check","reason":"compare upstream, patches, overlay and adapter inputs separately"}));
        if operation != "prepare" {
            steps.push(json!({"name":"compile","action":"check","reason":if compile_changed{"Rust/inputs changed or no previous build; Cargo checks required compilation"}else{"Rust/inputs unchanged; Cargo checks toolchain and dependency freshness"}}));
            steps.push(json!({"name":"validate","action":if validation_changed{"run"}else{"check"},"reason":if validation_changed{"validation inputs changed or no previous build"}else{"reuse only if compiled artifact hashes also match"}}));
        }
        if matches!(operation, "package" | "dev" | "deploy") {
            steps.push(json!({"name":"bundle","action":if unchanged {"check"}else{"run"},"reason":if unchanged {"verify existing bundle"}else{"source bundle inputs changed or no previous build"}}));
        }
        if operation == "package" {
            steps.push(json!({"name":"archive","action":"check","reason":"verify or create compressed distribution"}));
        }
        if operation == "deploy" {
            steps.push(json!({"name":"install","action":"run","reason":"install the bundle and place the application"}));
        }
    }
    Ok(
        json!({"operation":operation,"upstream":upstream,"offline":ctx.offline,"profile":ctx.profile,"inputs":inputs,"previous":previous,"steps":steps}),
    )
}
