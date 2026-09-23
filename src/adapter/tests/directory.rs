use super::*;
use vtabs_app::core::Tab;

fn tab(id: u64) -> Tab {
    Tab {
        id,
        title: format!("tab {id}"),
        ..Tab::default()
    }
}

fn listing(window: u64, id: u64, place: &str) -> ForeignTab {
    ForeignTab::new(window, &tab(id), None, place)
}

fn places(tabs: &[ForeignTab]) -> Vec<(u64, &str)> {
    tabs.iter()
        .map(|tab| (tab.id, tab.place.as_str()))
        .collect()
}

#[test]
fn other_reachable_windows_are_numbered_and_detached_tabs_follow() {
    publish(1, vec![listing(1, 10, "Home")]);
    publish(2, vec![listing(2, 20, "Work")]);
    publish(3, vec![listing(3, 30, "Home")]);
    let record = DetachedTab {
        listing: listing(9, 90, "devbox · detached"),
        tab: tab(90),
        domain: 4,
        remote: 1,
        show: false,
    };
    remember(record.clone());
    remember(record);
    assert_eq!(
        places(&foreign(1, |_| true)),
        [
            (20, "Work · window 2"),
            (30, "Home · window 3"),
            (90, "devbox · detached"),
        ]
    );
    assert_eq!(
        places(&foreign(1, |window| window != 2)),
        [(30, "Home · window 2"), (90, "devbox · detached")]
    );
}

#[test]
fn withdrawn_windows_are_no_longer_listed() {
    publish(40, vec![listing(40, 1, "Home")]);
    withdraw(40);
    assert!(foreign(41, |_| true).iter().all(|tab| tab.window != 40));
}
