# Boundaries

| Name                 | Value                                                                                                                                                                                                                                 |
| -------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| WezTerm              | Custom build required; follows latest `main`                                                                                                                                                                                          |
| Upstream changes     | Automatic fetch/build; incompatible source changes require an integration fix                                                                                                                                                         |
| Rust UI changes      | Require rebuilding the bundle                                                                                                                                                                                                         |
| Lua changes          | Configuration and semantic hooks reload without recompiling Rust                                                                                                                                                                      |
| Updates              | Managed launcher selects completed immutable bundles between launches                                                                                                                                                                 |
| Runtime tools        | Python, Git, Rust and platform build dependencies required for source updates                                                                                                                                                         |
| Rendering            | Retained Ratatui text with rounded surfaces and window framing                                                                                                                                                                        |
| Animation            | TachyonFX modifies cells; it is not a GPU shader language                                                                                                                                                                             |
| Live restoration     | Current upstream integration cannot verify a persistent mux incarnation; discovered tabs are rerouted after GUI restart/reconnect. Folder catalogs/settings persist; in-GUI workspace switches retain selection, assignments and pins |
| Tab tearoff          | Local moves preserve the whole split tree. Remote single-pane tabs can move; remote split-tab moves are rejected because upstream only exposes pane moves                                                                             |
| Reopen               | Launch metadata only; does not recover a dead process's execution state                                                                                                                                                               |
| Repository marker    | Owning mux resolves home and repository root; stock servers label remote directories as `/NAME`                                                                                                                                       |
| Multiple GUI clients | Normal upstream shared-mux focus and resize semantics still apply                                                                                                                                                                     |
| Remote servers       | Compatible upstream mux servers; distro glyphs and remote home/repository labels need a patched mux server                                                                                                                            |
| Windows              | Local, SSH and TLS mux; Unix-domain mux only where upstream supports it                                                                                                                                                               |
| GUI scenario samples | State/geometry/CPU instrumentation; not physical input-to-display or display frame pacing                                                                                                                                             |
| MacOS capture        | Opt-in fixture-window screenshots require available OS screen-capture access                                                                                                                                                          |

Private windows exclude live-tab persistence and reopen history. Catalog/settings changes are explicit shared edits. The sidebar never acquires a pane identity or changes split topology.

## Remote panes in local layouts

| Name                               | Value                                                                                                                                               |
| ---------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------- |
| `tls_clients[].local_pane_layout`  | `false`; set `true` on the mux that owns the displayed split tree                                                                                   |
| `unix_domains[].local_pane_layout` | `false`; same, e.g. with `proxy_command = { "ssh", "-T", HOST, "wezterm", "cli", "proxy" }`                                                         |
| `unix_domains[].proxy_command`     | Client-only; `wezterm-mux-server` neither listens on it nor exports it as `WEZTERM_UNIX_SOCKET`                                                     |
| `ssh_domains[].local_pane_layout`  | `false`; same, `multiplexing = "WezTerm"` only                                                                                                      |
| Attachment                         | Fresh remote shells; existing remote tabs/windows are not imported, only adopted                                                                    |
| Ownership                          | Local tabs and splits; each remote shell has an independent backing tab in the remote's `__backing:<host>` workspace                                |
| Hidden workspaces                  | `__detached`, `__backing:*`; a GUI never switches to them on its own                                                                                |
| CLI                                | `wezterm cli split-pane --pane-id ID --domain-name DOMAIN`                                                                                          |
| Adopt                              | `wezterm cli adopt-pane --domain-name DOMAIN --remote-pane-id ID [--window-id ID]`; moves the backing tab to `__backing:<host>`                     |
| Adoptable                          | `wezterm cli adopt-pane --domain-name DOMAIN --list`; not shown by any route, not relayed, not another host's backing tab                           |
| Lua capability                     | `wezterm.mux.supports_local_pane_layout`; `wezterm.mux.local_pane_layout_domains` = `{ "tls", "unix", "ssh" }`                                      |
| Client-pane metadata               | `pane:get_metadata().remote_pane_id`; ID on the immediately connected mux                                                                           |
| Activation                         | Updated GUI/CLI and owning mux server; restart required for existing domains                                                                        |
| Reattachment                       | Live TLS reconnects retain panes; unix/SSH disconnects close their panes; the next attach adopts orphaned `__backing:<host>` tabs into `__detached` |
| Server identity                    | Host name, executable and config path; routes to one server share adopted panes                                                                     |
