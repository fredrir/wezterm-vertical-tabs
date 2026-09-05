use crate::{Space, Tab};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchField {
    Domain,
    Host,
    User,
    Process,
    Cwd,
    Title,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutingRule {
    pub remote: Option<bool>,
    #[serde(default)]
    pub fields: Vec<(MatchField, Vec<String>)>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpaceTemplate {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub accent: Option<String>,
    pub rules: Vec<RoutingRule>,
}

pub fn validate_rules(rules: &[RoutingRule]) -> Result<(), String> {
    if rules.len() > 128
        || rules.iter().any(|r| {
            r.fields.len() > 6
                || r.fields
                    .iter()
                    .any(|(_, p)| p.len() > 64 || p.iter().any(|s| s.len() > 1024))
        })
    {
        Err("Routing rules exceed supported limits".into())
    } else {
        Ok(())
    }
}
fn fact(tab: &Tab, field: MatchField) -> &str {
    match field {
        MatchField::Domain => &tab.domain,
        MatchField::Host => &tab.host,
        MatchField::User => &tab.user,
        MatchField::Process => &tab.process,
        MatchField::Cwd => &tab.cwd,
        MatchField::Title => &tab.title,
    }
}
fn matches(rule: &RoutingRule, tab: &Tab) -> bool {
    rule.remote.is_none_or(|v| v == tab.remote)
        && rule.fields.iter().all(|(field, patterns)| {
            patterns.iter().any(|pattern| {
                let value = fact(tab, *field);
                glob(pattern, value)
                    || (*field == MatchField::Cwd
                        && !pattern.contains('*')
                        && value
                            .strip_prefix(pattern)
                            .is_some_and(|s| s.starts_with('/')))
            })
        })
}
/// Anchored wildcard matching; only `*` is special, so paths do not become regex programs.
fn glob(pattern: &str, value: &str) -> bool {
    let (p, v) = (pattern.as_bytes(), value.as_bytes());
    let (mut pi, mut vi, mut star, mut retry) = (0, 0, None, 0);
    while vi < v.len() {
        if pi < p.len() && p[pi] != b'*' && p[pi] == v[vi] {
            pi += 1;
            vi += 1;
        } else if pi < p.len() && p[pi] == b'*' {
            star = Some(pi);
            pi += 1;
            retry = vi;
        } else if let Some(s) = star {
            retry += 1;
            vi = retry;
            pi = s + 1;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == b'*' {
        pi += 1;
    }
    pi == p.len()
}
pub(crate) fn expand(template: &str, tab: &Tab) -> Option<String> {
    let mut out = String::new();
    let mut rest = template;
    while let Some(pos) = rest.find('$') {
        out.push_str(&rest[..pos]);
        rest = &rest[pos + 1..];
        let n = rest.bytes().take_while(u8::is_ascii_alphabetic).count();
        if n == 0 {
            out.push('$');
            continue;
        }
        let key = &rest[..n];
        rest = &rest[n..];
        let value = match key {
            "domain" => &tab.domain,
            "host" => &tab.host,
            "user" => &tab.user,
            "proc" | "process" => &tab.process,
            "cwd" => &tab.cwd,
            "title" => &tab.title,
            _ => return None,
        };
        if value.is_empty() {
            return None;
        }
        out.push_str(value);
    }
    out.push_str(rest);
    if out.is_empty() || out.len() > 128 || out.chars().any(char::is_control) {
        None
    } else {
        Some(out)
    }
}
pub(crate) fn route(spaces: &[Space], templates: &[SpaceTemplate], tab: &Tab) -> Option<String> {
    spaces
        .iter()
        .find(|s| !s.rules.is_empty() && s.rules.iter().any(|r| matches(r, tab)))
        .map(|s| s.id.clone())
        .or_else(|| {
            templates.iter().find_map(|t| {
                t.rules
                    .iter()
                    .any(|r| matches(r, tab))
                    .then(|| expand(&t.id, tab))
                    .flatten()
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn anchored_star_handles_backtracking() {
        assert!(glob("*a", "aba"));
        assert!(!glob("a", "aba"));
        assert!(glob("工作/*", "工作/例"));
    }
    #[test]
    fn directory_rules_respect_component_boundaries() {
        let r = RoutingRule {
            remote: None,
            fields: vec![(MatchField::Cwd, vec!["/work".into()])],
        };
        assert!(matches(
            &r,
            &Tab {
                cwd: "/work/repo".into(),
                ..Tab::default()
            }
        ));
        assert!(!matches(
            &r,
            &Tab {
                cwd: "/worker".into(),
                ..Tab::default()
            }
        ));
    }
}
