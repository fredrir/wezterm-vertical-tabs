//! Launcher entries for the sidebar, built as WezTerm's launcher overlay builds them.
use crate::commands::derive_command_from_key_assignment;
use crate::inputmap::InputMap;
use config::keyassignment::{KeyAssignment, LauncherFlags, SpawnCommand, SpawnTabDomain};
use mux::domain::{DomainId, DomainState};
use mux::tab::TabId;
use mux::Mux;
use std::collections::BTreeMap;

#[derive(Clone)]
pub struct Entry {
    pub label: String,
    pub action: KeyAssignment,
    pub tab_id: Option<TabId>,
}

pub struct LauncherTabEntry {
    pub title: String,
    pub tab_id: TabId,
    pub pane_count: Option<usize>,
}

pub struct LauncherDomainEntry {
    pub domain_id: DomainId,
    pub name: String,
    pub state: DomainState,
    pub label: String,
}

pub struct LauncherArgs {
    pub flags: LauncherFlags,
    pub domains: Vec<LauncherDomainEntry>,
    pub tabs: Vec<LauncherTabEntry>,
    pub domain_id_of_current_tab: DomainId,
    pub active_workspace: String,
    pub workspaces: Vec<String>,
}

pub struct LauncherEntries {
    pub entries: Vec<Entry>,
    pub selected: usize,
}

impl LauncherArgs {
    /// Must be called on the Mux thread!
    pub async fn new(
        flags: LauncherFlags,
        visible_tabs: Vec<TabId>,
        domain_id_of_current_tab: DomainId,
    ) -> Self {
        let mux = Mux::get();

        let active_workspace = mux.active_workspace();

        let workspaces = if flags.contains(LauncherFlags::WORKSPACES) {
            mux.iter_workspaces()
        } else {
            vec![]
        };

        let tabs = if flags.contains(LauncherFlags::TABS) {
            visible_tabs
                .into_iter()
                .filter_map(|id| mux.get_tab(id))
                .map(|tab| {
                    let tab_title = tab.get_title();
                    let title = if tab_title.is_empty() {
                        tab.get_active_pane()
                            .map(|pane| pane.get_title())
                            .unwrap_or_default()
                    } else {
                        tab_title
                    };
                    LauncherTabEntry {
                        title,
                        tab_id: tab.tab_id(),
                        pane_count: tab.count_panes(),
                    }
                })
                .collect()
        } else {
            vec![]
        };

        let domains = if flags.contains(LauncherFlags::DOMAINS) {
            let mut domains = mux.iter_domains();
            domains.sort_by(|a, b| {
                let a_state = a.state();
                let b_state = b.state();
                if a_state != b_state {
                    use std::cmp::Ordering;
                    return if a_state == DomainState::Attached {
                        Ordering::Less
                    } else {
                        Ordering::Greater
                    };
                }
                a.domain_id().cmp(&b.domain_id())
            });
            domains.retain(|dom| dom.spawnable());
            let mut d = vec![];
            for dom in domains.into_iter() {
                let name = dom.domain_name();
                let label = dom.domain_label().await;
                let label = if name == label || label == "" {
                    format!("domain `{}`", name)
                } else {
                    format!("domain `{}` - {}", name, label)
                };
                d.push(LauncherDomainEntry {
                    domain_id: dom.domain_id(),
                    name: name.to_string(),
                    state: dom.state(),
                    label,
                });
            }
            d
        } else {
            vec![]
        };

        Self {
            flags,
            domains,
            tabs,
            domain_id_of_current_tab,
            active_workspace,
            workspaces,
        }
    }

    pub fn build_entries(
        &self,
        config: &config::ConfigHandle,
        initial_choice_idx: usize,
    ) -> LauncherEntries {
        let mut result = LauncherEntries {
            entries: vec![],
            selected: initial_choice_idx,
        };
        if self.flags.contains(LauncherFlags::LAUNCH_MENU_ITEMS) {
            for item in &config.launch_menu {
                result.entries.push(Entry {
                    tab_id: None,
                    label: match item.label.as_ref() {
                        Some(label) => label.to_string(),
                        None => match item.args.as_ref() {
                            Some(args) => args.join(" "),
                            None => "(default shell)".to_string(),
                        },
                    },
                    action: KeyAssignment::SpawnCommandInNewTab(item.clone()),
                });
            }
        }

        for domain in &self.domains {
            let entry = if domain.state == DomainState::Attached {
                Entry {
                    tab_id: None,
                    label: format!("New Tab ({})", domain.label),
                    action: KeyAssignment::SpawnCommandInNewTab(SpawnCommand {
                        domain: SpawnTabDomain::DomainName(domain.name.to_string()),
                        ..SpawnCommand::default()
                    }),
                }
            } else {
                Entry {
                    tab_id: None,
                    label: format!("Attach {}", domain.label),
                    action: KeyAssignment::AttachDomain(domain.name.to_string()),
                }
            };

            // Enter right away spawns in the current tab's domain.
            if domain.domain_id == self.domain_id_of_current_tab {
                result.selected = result.entries.len();
            }
            result.entries.push(entry);
        }

        if self.flags.contains(LauncherFlags::WORKSPACES) {
            for ws in &self.workspaces {
                if *ws != self.active_workspace {
                    result.entries.push(Entry {
                        tab_id: None,
                        label: format!("Switch to workspace: `{}`", ws),
                        action: KeyAssignment::SwitchToWorkspace {
                            name: Some(ws.clone()),
                            spawn: None,
                        },
                    });
                }
            }
            result.entries.push(Entry {
                tab_id: None,
                label: format!(
                    "Create new Workspace (current is `{}`)",
                    self.active_workspace
                ),
                action: KeyAssignment::SwitchToWorkspace {
                    name: None,
                    spawn: None,
                },
            });
        }

        for tab in &self.tabs {
            result.entries.push(Entry {
                tab_id: Some(tab.tab_id),
                label: match tab.pane_count {
                    Some(pane_count) => format!("{}. {pane_count} panes", tab.title),
                    None => format!("{}.", tab.title),
                },
                action: KeyAssignment::Nop,
            });
        }

        if self.flags.contains(LauncherFlags::COMMANDS) {
            let commands = crate::commands::CommandDef::expanded_commands(config);
            for cmd in commands {
                if matches!(
                    &cmd.action,
                    KeyAssignment::ActivateTabRelative(_) | KeyAssignment::ActivateTab(_)
                ) {
                    // Filter out some noisy, repetitive entries
                    continue;
                }
                result.entries.push(Entry {
                    tab_id: None,
                    label: format!("{}. {}", cmd.brief, cmd.doc),
                    action: cmd.action,
                });
            }
        }

        // Grab interesting key assignments and show those as a kind of command palette
        if self.flags.contains(LauncherFlags::KEY_ASSIGNMENTS) {
            let input_map = InputMap::new(config);
            let mut key_entries: Vec<Entry> = vec![];
            // Give a consistent order to the entries
            let keys: BTreeMap<_, _> = input_map.keys.default.into_iter().collect();
            for ((keycode, mods), entry) in keys {
                if matches!(
                    &entry.action,
                    KeyAssignment::ActivateTabRelative(_) | KeyAssignment::ActivateTab(_)
                ) {
                    // Filter out some noisy, repetitive entries
                    continue;
                }
                if key_entries
                    .iter()
                    .find(|ent| ent.action == entry.action)
                    .is_some()
                {
                    // Avoid duplicate entries
                    continue;
                }

                let label = match derive_command_from_key_assignment(&entry.action) {
                    Some(cmd) => format!("{}. {}", cmd.brief, cmd.doc),
                    None => format!(
                        "{:?} ({} {})",
                        entry.action,
                        mods.to_string(),
                        keycode.to_string().escape_debug()
                    ),
                };

                key_entries.push(Entry {
                    tab_id: None,
                    label,
                    action: entry.action,
                });
            }
            key_entries.sort_by(|a, b| a.label.cmp(&b.label));
            result.entries.append(&mut key_entries);
        }
        result
    }
}
