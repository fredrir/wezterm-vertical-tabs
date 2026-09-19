use super::*;
use std::fs;

#[test]
fn repository_roots_cover_ancestors_and_cache_every_directory() {
    let root = std::env::temp_dir().join(format!("vtabs-repos-{}", std::process::id()));
    let repo = root.join("repo");
    let nested = repo.join("src/ui");
    let plain = root.join("plain");
    fs::create_dir_all(&nested).unwrap();
    fs::create_dir_all(&plain).unwrap();
    fs::create_dir_all(repo.join(".git")).unwrap();
    let repository = repo.to_str().unwrap();

    let mut repos = repos::Repos::default();
    assert_eq!(repos.root(nested.to_str().unwrap()), Some(repository));
    assert_eq!(repos.root(nested.to_str().unwrap()), Some(repository));
    assert_eq!(repos.root(repository), Some(repository));
    assert_eq!(repos.root(plain.to_str().unwrap()), None);
    assert_eq!(repos.root(plain.to_str().unwrap()), None);
    assert_eq!(repos.root(""), None);
    fs::remove_dir_all(&root).unwrap();
}
