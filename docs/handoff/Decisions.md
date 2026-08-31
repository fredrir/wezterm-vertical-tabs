# Decisions

## Fredrir's Decisions

- Zen frame: background wraps the whole window, main terminal only as its own surface, thin **rounded** borders — "That is important". Default-on not yet approved.
- Sidebar keeps its own distinct background colour (fold-back after removal).
- Only visible edge: the sidebar↔editor split line.
- Config-as-code wins over the GUI settings page (LOCKED badges).
- Hover: `pane_focus_follows_mouse` default + press mode both supported.
- Close on press, not hold.
- macOS: `INTEGRATED_BUTTONS|RESIZE` when unset; strip icons sized/centred on the traffic-light row.
- No `·` separators; no thick left border on the active tab.
- Terminal padding restored (`edge_to_edge = "sides"`); sidebar padding x > y.
- `meta` (cwd/socket paths) off by default.
- Splitting from the sidebar pane must be impossible.
- Code: de-duplicated, modularized, separated concerns, DRY.
- Opus agent team, session lead orchestrates; ask before `git push`.

## Agent's Decisions

| Decision                                                 | Rationale                                                                                                                    | Alternatives Considered                                                   |
| -------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------- |
| Zen frame = generated PNG `config.background` layer      | WezTerm has no native rounded window borders; default-bg cells go transparent over a layer, giving the rounded card for free | Per-cell painting (no rounding), window-decoration hacks (platform-bound) |
| Hand-rolled fixed-Huffman PNG encoder in `backend`       | 108KB/90ms, zero new crates                                                                                                  | `image`/`png` crates (dependency weight)                                  |
| Backend-clocked animation (`anim`)                       | `wezterm.time.call_after` leaks a registry entry per call                                                                    | `call_after` (leaks), no animation                                        |
| Sidebar trust = echoed process-minted token, rank ladder | Title strings are forgeable; only an echo of a token we minted proves identity                                               | Trust marker title alone (spoofable)                                      |
| Refactor gated on pinned byte-identical frame baseline   | "Behaviour-neutral" is provable, not asserted                                                                                | Reviewer judgement only                                                   |
| `geometry.lua` sole `AdjustPaneSize` caller              | `adjust_x_size` deals deltas alternately to split children; one owner keeps the invariant checkable                          | Ad-hoc resize calls per feature                                           |
| e2e probes (`probe_active_title`) instead of glyph greps | Glyphs change with design; probes don't                                                                                      | Grep for ▎ (broke once already)                                           |
| e2e timeouts local 600 / mux 900                         | mux fails by hanging to timeout under load; short timeouts create false reds                                                 | 480 flat (flaked under `cargo build` contention)                          |
