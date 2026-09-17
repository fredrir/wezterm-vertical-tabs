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
