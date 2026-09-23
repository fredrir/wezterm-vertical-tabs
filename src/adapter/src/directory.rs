use std::{cell::RefCell, collections::BTreeMap};
use vtabs_app::{core, ui::ForeignTab};

#[derive(Clone, Debug)]
pub struct DetachedTab {
    pub listing: ForeignTab,
    pub tab: core::Tab,
    pub domain: usize,
    pub remote: usize,
    pub show: bool,
}

#[derive(Default)]
struct Directory {
    windows: BTreeMap<usize, Vec<ForeignTab>>,
    detached: Vec<DetachedTab>,
}

thread_local! {
    static DIRECTORY: RefCell<Directory> = Default::default();
}

pub fn publish(window: usize, tabs: Vec<ForeignTab>) {
    DIRECTORY.with_borrow_mut(|directory| directory.windows.insert(window, tabs));
}

pub fn withdraw(window: usize) {
    DIRECTORY.with_borrow_mut(|directory| directory.windows.remove(&window));
}

pub fn remember(record: DetachedTab) {
    DIRECTORY.with_borrow_mut(|directory| {
        if !directory
            .detached
            .iter()
            .any(|known| known.tab.id == record.tab.id)
        {
            directory.detached.push(record);
        }
    });
}

pub fn detached<R>(f: impl FnOnce(&mut Vec<DetachedTab>) -> R) -> R {
    DIRECTORY.with_borrow_mut(|directory| f(&mut directory.detached))
}

pub fn foreign(window: usize, reachable: impl Fn(usize) -> bool) -> Vec<ForeignTab> {
    DIRECTORY.with_borrow(|directory| {
        directory
            .windows
            .iter()
            .filter(|(id, _)| **id == window || reachable(**id))
            .enumerate()
            .filter(|(_, (id, _))| **id != window)
            .flat_map(|(number, (_, tabs))| {
                tabs.iter().map(move |tab| ForeignTab {
                    place: format!("{} · window {}", tab.place, number + 1),
                    ..tab.clone()
                })
            })
            .chain(
                directory
                    .detached
                    .iter()
                    .map(|record| record.listing.clone()),
            )
            .collect()
    })
}

#[cfg(test)]
#[path = "../tests/directory.rs"]
mod tests;
