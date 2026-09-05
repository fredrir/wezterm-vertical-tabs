# Boundaries

| Name | Value |
| --- | --- |
| WezTerm | Custom native build required; follows latest `main` |
| Upstream changes | Automatic fetch/build; incompatible source changes require an integration fix |
| Rust UI changes | Require rebuilding the native bundle |
| Lua changes | Configuration and semantic hooks reload without recompiling Rust |
| Updates | Managed launcher selects completed immutable bundles between launches |
| Runtime tools | Python, Git, Rust and platform build dependencies required for source updates |
| Rendering | Retained Ratatui text with native rounded surfaces and window framing |
| Animation | TachyonFX modifies cells; it is not a GPU shader language |
| Live restoration | Current upstream integration cannot verify a persistent mux incarnation; discovered tabs are rerouted after GUI restart/reconnect. Folder catalogs/settings persist; in-GUI workspace switches retain selection, assignments and pins |
| Tab tearoff | Local moves preserve the whole split tree. Remote single-pane tabs can move; remote split-tab moves are rejected because upstream only exposes pane moves |
| Reopen | Launch metadata only; does not recover a dead process's execution state |
| Multiple GUI clients | Normal upstream shared-mux focus and resize semantics still apply |
| Remote servers | Compatible upstream mux servers; no remote SQLite helper required |
| Windows | Local, SSH and TLS mux; Unix-domain mux only where upstream supports it |
| GUI scenario samples | State/geometry/CPU instrumentation; not physical input-to-display or display frame pacing |
| macOS capture | Opt-in fixture-window screenshots require available OS screen-capture access |

Private windows exclude live-tab persistence and reopen history. Catalog/settings changes are explicit shared edits. The sidebar never acquires a pane identity or changes split topology.
