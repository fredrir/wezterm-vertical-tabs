use super::*;

fn tab(cwd: &str, repo_root: Option<&str>) -> Tab {
    Tab {
        id: 1,
        cwd: cwd.into(),
        repo_root: repo_root.map(str::to_owned),
        ..Tab::default()
    }
}

#[test]
fn location_names_the_repository_root_for_nested_directories() {
    assert_eq!(
        tab("/srv/repo/src/ui", Some("/srv/repo"))
            .location(None)
            .as_deref(),
        Some("repo")
    );
    assert_eq!(
        tab(
            "/Users/fredrir/dotfiles/scripts",
            Some("/Users/fredrir/dotfiles")
        )
        .location(Some("/Users/fredrir"))
        .as_deref(),
        Some("dotfiles")
    );
    assert_eq!(
        tab("/Users/fredrir/dotfiles", Some("/Users/fredrir/dotfiles"))
            .location(Some("/Users/fredrir"))
            .as_deref(),
        Some("dotfiles")
    );
}

#[test]
fn location_marks_the_home_directory_and_absolute_directories() {
    assert_eq!(
        tab("/Users/fredrir", None)
            .location(Some("/Users/fredrir/"))
            .as_deref(),
        Some("~/")
    );
    assert_eq!(
        tab("/Users/fredrir/Downloads", None)
            .location(Some("/Users/fredrir"))
            .as_deref(),
        Some("~/Downloads")
    );
    assert_eq!(
        tab("/etc/nginx", None).location(None).as_deref(),
        Some("/nginx")
    );
    assert_eq!(tab("/", None).location(None).as_deref(), Some("/"));
    assert_eq!(
        tab("/Users/fredrirx", None)
            .location(Some("/Users/fredrir"))
            .as_deref(),
        Some("/fredrirx")
    );
    assert_eq!(tab("", None).location(Some("/Users/fredrir")), None);
}

#[test]
fn location_labels_against_the_owning_machines_home() {
    let remote = |cwd: &str, repo_root: Option<&str>| Tab {
        home: Some("/home/fredrir".into()),
        ..tab(cwd, repo_root)
    };
    let local = Some("/Users/fredrir");
    assert_eq!(
        remote("/home/fredrir/Downloads", None)
            .location(local)
            .as_deref(),
        Some("~/Downloads")
    );
    assert_eq!(
        remote("/home/fredrir", None).location(local).as_deref(),
        Some("~/")
    );
    assert_eq!(
        remote(
            "/home/fredrir/dotfiles/nvim",
            Some("/home/fredrir/dotfiles")
        )
        .location(local)
        .as_deref(),
        Some("dotfiles")
    );
    assert_eq!(
        remote("/Users/fredrir/Downloads", None)
            .location(local)
            .as_deref(),
        Some("/Downloads")
    );
    assert_eq!(
        tab("/home/fredrir/Downloads", None)
            .location(local)
            .as_deref(),
        Some("/Downloads")
    );
}

#[test]
fn pane_labels_against_the_owning_machines_home() {
    let pane = TabPane {
        cwd: "/root/src".into(),
        home: Some("/root".into()),
        title: "zsh".into(),
        ..TabPane::default()
    };
    assert_eq!(pane.label(Some("/Users/fredrir")), "~/src");
    assert_eq!(
        TabPane {
            home: None,
            ..pane.clone()
        }
        .label(Some("/Users/fredrir")),
        "/src"
    );
}
