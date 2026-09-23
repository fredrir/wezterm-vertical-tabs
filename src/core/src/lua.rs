//! Lua source for JSON-compatible values: generated contracts and the managed settings file.
use serde_json::Value;
use std::fmt::Write;

const WIDTH: usize = 120;
const KEYWORDS: &[&str] = &[
    "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "goto", "if", "in",
    "local", "nil", "not", "or", "repeat", "return", "then", "true", "until", "while",
];

pub fn string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => write!(out, "\\u{{{:x}}}", c as u32).unwrap(),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn key(name: &str) -> String {
    let identifier = name
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !KEYWORDS.contains(&name);
    if identifier {
        name.into()
    } else {
        format!("[{}]", string(name))
    }
}

fn inline(value: &Value) -> String {
    match value {
        Value::Null => "nil".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => string(s),
        Value::Array(items) if items.is_empty() => "{}".into(),
        Value::Array(items) => format!(
            "{{ {} }}",
            items.iter().map(inline).collect::<Vec<_>>().join(", ")
        ),
        Value::Object(fields) if fields.is_empty() => "{}".into(),
        Value::Object(fields) => format!(
            "{{ {} }}",
            fields
                .iter()
                .map(|(k, v)| format!("{} = {}", key(k), inline(v)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn block(value: &Value, indent: usize, column: usize) -> String {
    let flat = inline(value);
    if column + flat.chars().count() + usize::from(indent > 0) <= WIDTH {
        return flat;
    }
    let entries: Vec<_> = match value {
        Value::Array(items) => items.iter().map(|item| (String::new(), item)).collect(),
        Value::Object(fields) => fields
            .iter()
            .map(|(name, value)| (format!("{} = ", key(name)), value))
            .collect(),
        _ => return flat,
    };
    let mut out = String::from("{\n");
    for (prefix, value) in entries {
        let column = indent + 2 + prefix.chars().count();
        writeln!(
            out,
            "{}{prefix}{},",
            " ".repeat(indent + 2),
            block(value, indent + 2, column)
        )
        .unwrap();
    }
    write!(out, "{}}}", " ".repeat(indent)).unwrap();
    out
}

/// A Lua expression for `value`, wrapped at 120 columns after `column` characters.
pub fn source(value: &Value, column: usize) -> String {
    block(value, 0, column)
}
