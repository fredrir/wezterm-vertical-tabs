//! Generate optional Lua types and a machine-readable schema from the Rust authority.
use serde_json::{Map, Value};
use std::fmt::Write;
use vtabs_core::{SettingKind, Settings, lua, settings};

const PREFIX: &str = "Tabs";

fn class_name(reference: &str) -> String {
    format!("{PREFIX}{}", reference.rsplit('/').next().unwrap())
}

fn literal(value: &Value) -> String {
    match value {
        Value::String(text) => format!("'{text}'"),
        other => other.to_string(),
    }
}

fn nullable(ty: String) -> String {
    if ty.contains(['|', ' ', '?']) {
        format!("{ty}|nil")
    } else {
        format!("{ty}?")
    }
}

fn element(ty: String) -> String {
    if ty.contains(['|', '?']) {
        format!("({ty})")
    } else {
        ty
    }
}

fn is_null(schema: &Value) -> bool {
    schema.get("type").and_then(Value::as_str) == Some("null")
}

fn fields(object: &Map<String, Value>, separator: &str) -> Vec<(String, String)> {
    let required: Vec<&str> = object
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    object
        .get("properties")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .map(|(name, schema)| {
            let optional = !required.contains(&name.as_str());
            let mut ty = lua_type(schema);
            if optional && let Some(inner) = ty.strip_suffix('?').or(ty.strip_suffix("|nil")) {
                ty = inner.into();
            }
            let marker = if optional { "?" } else { "" };
            (
                format!("{name}{marker}{separator}{ty}"),
                description(schema),
            )
        })
        .collect()
}

fn description(schema: &Value) -> String {
    schema
        .get("description")
        .and_then(Value::as_str)
        .map(|text| text.split_whitespace().collect::<Vec<_>>().join(" "))
        .unwrap_or_default()
}

fn primitive(name: &str, object: &Map<String, Value>) -> String {
    match name {
        "string" => "string".into(),
        "integer" => "integer".into(),
        "number" => "number".into(),
        "boolean" => "boolean".into(),
        "null" => "nil".into(),
        "array" => match (object.get("prefixItems"), object.get("items")) {
            (Some(Value::Array(items)), _) => format!(
                "{{ {} }}",
                items
                    .iter()
                    .enumerate()
                    .map(|(index, item)| format!("[{}]: {}", index + 1, lua_type(item)))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            (_, Some(items)) => format!("{}[]", element(lua_type(items))),
            _ => "any[]".into(),
        },
        "object" => match (object.get("properties"), object.get("additionalProperties")) {
            (Some(_), _) => format!(
                "{{ {} }}",
                fields(object, ": ")
                    .into_iter()
                    .map(|(field, _)| field)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            (_, Some(values @ Value::Object(_))) => format!("table<string, {}>", lua_type(values)),
            _ => "table".into(),
        },
        _ => "any".into(),
    }
}

fn lua_type(schema: &Value) -> String {
    let Some(object) = schema.as_object().filter(|object| !object.is_empty()) else {
        return "any".into();
    };
    if let Some(reference) = object.get("$ref").and_then(Value::as_str) {
        return class_name(reference);
    }
    if let Some(value) = object.get("const") {
        return literal(value);
    }
    if let Some(values) = object.get("enum").and_then(Value::as_array) {
        return values.iter().map(literal).collect::<Vec<_>>().join("|");
    }
    if let Some(variants) = object
        .get("oneOf")
        .or_else(|| object.get("anyOf"))
        .and_then(Value::as_array)
    {
        let types = variants
            .iter()
            .filter(|variant| !is_null(variant))
            .map(lua_type)
            .collect::<Vec<_>>()
            .join("|");
        return if variants.iter().any(is_null) {
            nullable(types)
        } else {
            types
        };
    }
    match object.get("type") {
        Some(Value::String(name)) => primitive(name, object),
        Some(Value::Array(names)) => {
            let types = names
                .iter()
                .filter_map(Value::as_str)
                .filter(|name| *name != "null")
                .map(|name| primitive(name, object))
                .collect::<Vec<_>>()
                .join("|");
            if names.iter().any(|name| name == "null") {
                nullable(types)
            } else {
                types
            }
        }
        _ => "any".into(),
    }
}

fn documented(out: &mut String, text: &str) {
    if !text.is_empty() {
        writeln!(out, "--- {text}").unwrap();
    }
}

fn definitions() -> Map<String, Value> {
    let mut generator = schemars::SchemaGenerator::default();
    generator.subschema_for::<vtabs_core::Managed>();
    generator.subschema_for::<vtabs_core::Tab>();
    generator.subschema_for::<vtabs_core::WindowContext>();
    generator.subschema_for::<vtabs_core::Action>();
    generator.subschema_for::<settings::MenuEntry>();
    let mut definitions = generator.take_definitions(true);
    // Settings are annotated from their descriptors, which carry ranges and descriptions.
    definitions.remove("Settings");
    definitions
}

fn setting_type(kind: &SettingKind) -> String {
    match kind {
        SettingKind::Bool => "boolean".into(),
        SettingKind::Number { .. } => "integer".into(),
        SettingKind::Text => "string".into(),
        SettingKind::Color => "string".into(),
        SettingKind::Colors => "table<string, string>".into(),
        SettingKind::Choice(choices) => choices
            .iter()
            .map(|c| format!("'{c}'"))
            .collect::<Vec<_>>()
            .join("|"),
        SettingKind::Object => "table<string, string>".into(),
        SettingKind::List => "TabsMenuEntry[]".into(),
    }
}

fn settings_class(out: &mut String, name: &str, theme_only: bool) {
    writeln!(out, "---@class {name}").unwrap();
    for option in settings::descriptors() {
        if !theme_only || settings::is_theme(option.key) {
            writeln!(
                out,
                "---@field {}? {} {}",
                option.key,
                setting_type(&option.kind),
                option.description
            )
            .unwrap();
        }
    }
    out.push('\n');
}

const API: &str = r#"---@class TabsHooks
---@field title? fun(tab: TabsTab): string? Display title
---@field routing? fun(tab: TabsTab): string? Existing space ID
---@field filter? fun(tab: TabsTab): boolean? Visibility
---@field theme? fun(context: TabsWindowContext): TabsTheme? Theme-color overrides
---@field footer? fun(context: TabsWindowContext): string|string[]|nil Rows shown above spaces

---@class TabsOptions
---@field profile? string Shared catalog scope; default `default`
---@field settings? TabsSettings Owned by this config; read-only in the settings UI
---@field spaces? TabsSpace[] Owned by this config; listed before spaces created in the UI
---@field templates? TabsSpaceTemplate[] Dynamic spaces derived from tab metadata
---@field hooks? TabsHooks
---@field settings_file? string File the settings UI writes; default `wezterm.config_dir .. "/vtabs_settings.lua"`
"#;

fn types() -> String {
    let mut out = String::from("-- Generated from vtabs-core; edit the Rust types.\n---@meta\n\n");
    settings_class(&mut out, "TabsSettings", false);
    settings_class(&mut out, "TabsTheme", true);
    for (name, schema) in definitions() {
        let object = schema.as_object().unwrap();
        documented(&mut out, &description(&schema));
        if object.contains_key("properties")
            && object.get("type").and_then(Value::as_str) == Some("object")
        {
            writeln!(out, "---@class {PREFIX}{name}").unwrap();
            for (field, text) in fields(object, " ") {
                writeln!(out, "{}", format!("---@field {field} {text}").trim_end()).unwrap();
            }
        } else {
            writeln!(out, "---@alias {PREFIX}{name} {}", lua_type(&schema)).unwrap();
        }
        out.push('\n');
    }
    out.push_str(API);
    out
}

fn generate(format: &str) -> Result<String, String> {
    match format {
        "json" => Ok(format!(
            "{}\n",
            serde_json::to_string_pretty(&Settings::schema()).unwrap()
        )),
        "lua" => Ok(format!(
            "-- Generated by cargo run -p vtabs-core --bin gen-schema -- lua\nreturn {}\n",
            lua::source(&Settings::schema(), 7)
        )),
        "types" => Ok(types()),
        "markdown" => {
            let defaults = Settings::default();
            let mut out = String::from(
                "<!-- Generated by cargo run -p vtabs-core --bin gen-schema -- --write markdown docs/options.md -->\n\n",
            );
            let mut rows = vec![["Name".into(), "Default".into(), "Value".into()]];
            for option in settings::descriptors() {
                let kind = match option.kind {
                    SettingKind::Bool => "Boolean".into(),
                    SettingKind::Number { min, max } => format!("Integer {min}–{max}"),
                    SettingKind::Text => "String or null".into(),
                    SettingKind::Color => "Hex color `#RRGGBB`".into(),
                    SettingKind::Colors => "Object of hex colors".into(),
                    SettingKind::Choice(choices) => choices
                        .iter()
                        .map(|value| format!("`{value}`"))
                        .collect::<Vec<_>>()
                        .join(", "),
                    SettingKind::Object => "Object".into(),
                    SettingKind::List => "List".into(),
                };
                let default = serde_json::to_string(&defaults.get(option.key).unwrap()).unwrap();
                let cell = |value: &str| value.replace('|', "\\|").replace('\n', "<br>");
                rows.push([
                    format!("`{}`", option.key),
                    format!("`{}`", cell(&default)),
                    format!("{}. {}", cell(&kind), cell(option.description)),
                ]);
            }
            let widths: [usize; 3] = std::array::from_fn(|column| {
                rows.iter()
                    .map(|row| row[column].chars().count())
                    .max()
                    .unwrap()
            });
            rows.insert(1, widths.map(|width| "-".repeat(width)));
            for row in rows {
                out.push('|');
                for (cell, width) in row.into_iter().zip(widths) {
                    write!(&mut out, " {cell:width$} |").unwrap();
                }
                out.push('\n');
            }
            Ok(out)
        }
        _ => Err(
            "usage: gen-schema [json|lua|types|markdown] or gen-schema --check|--write FORMAT PATH"
                .into(),
        ),
    }
}
fn run() -> Result<(), String> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args
        .first()
        .is_some_and(|a| a == "--check" || a == "--write")
    {
        if args.len() != 3 {
            return Err("usage: gen-schema --check|--write FORMAT PATH".into());
        }
        let content = generate(&args[1])?;
        if args[0] == "--write" {
            std::fs::write(&args[2], content).map_err(|e| e.to_string())?;
        } else {
            let actual = std::fs::read_to_string(&args[2]).map_err(|e| e.to_string())?;
            if actual.replace("\r\n", "\n") != content {
                return Err(format!(
                    "{} is stale; regenerate it with gen-schema --write {} {}",
                    args[2], args[1], args[2]
                ));
            }
        }
    } else {
        print!(
            "{}",
            generate(args.first().map(String::as_str).unwrap_or("json"))?
        );
    }
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
