fn trimmed_path(path: &str) -> &str {
    path.trim_end_matches('/')
}

fn dir_name(path: &str) -> &str {
    let path = trimmed_path(path);
    path.rsplit('/')
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or(path)
}

fn is_under_home(path: &str, home: &str) -> bool {
    let home = trimmed_path(home);
    !home.is_empty()
        && trimmed_path(path)
            .strip_prefix(home)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

fn is_home(path: &str, home: &str) -> bool {
    trimmed_path(path) == trimmed_path(home)
}

/// Labels shorten a directory for display; the path itself is never rewritten.
pub(crate) fn label(cwd: &str, repo_root: Option<&str>, home: Option<&str>) -> Option<String> {
    if cwd.is_empty() {
        return None;
    }
    if let Some(repo) = repo_root {
        return Some(dir_name(repo).to_owned());
    }
    let home = home.filter(|home| is_under_home(cwd, home));
    if home.is_some_and(|home| is_home(cwd, home)) {
        return Some("~/".into());
    }
    let prefix = if home.is_some() { "~/" } else { "/" };
    Some(format!("{prefix}{}", dir_name(cwd)))
}
