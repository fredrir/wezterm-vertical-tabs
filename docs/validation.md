# Native validation

| Name | Value |
| --- | --- |
| Date | 2026-09-05 |
| Host | Apple M5 Pro, macOS arm64, release builds |
| Upstream tested | `d4f5c4878a7f72b3fa53ff963a1fea790810d31a`; diagnostic observation, builds resolve latest main |
| Renderer | Actual WezTerm GUI; Ratatui 0.30.2, TachyonFX 0.25.1, upstream Termwiz |
| Reference | Stock GUI from the same upstream source, equivalent content viewport |
| Project checks | 80 Rust tests, format, Clippy, generated Lua/schema/docs checks |
| Integration checks | 20 GUI adapter/host tests, 3 mux topology race tests, 15 build/install/update tests |

| Session | Resize steps | GUI state samples | Result |
| --- | ---: | ---: | --- |
| Local | 12 | 287 | Passed |
| Unix mux | 60 | 457 | Passed |
| SSH mux | 36 | 376 | Passed; isolated localhost SSH server and keys |

| Check | Result |
| --- | --- |
| Dense resize/reversal | Sidebar reservation and foreground/background tab sizes remain coherent during sequence sampling |
| Split integrity | Pane identities and topology preserved; sizes match stock at equivalent content dimensions |
| Extreme shrinking | Stock and native share minimum-cell rounding and per-split minimum widths; a 49/50 split can restore as 50/49 after clamping to minimum content width |
| Native navigation | Existing indexed/negative tab actions use visible tabs; no replacement keybindings |
| Spaces | Empty space, native new tab, assignment, workspace out/back with retained selection and pins |
| Layout edges | Left/right, expanded/collapsed/hidden rail, padding, integrated controls, tiny windows; font/fullscreen/zoom round trips checked in all three session types |
| Hooks/effects | All five semantic hooks and finite TachyonFX transitions enabled |
| Idle | Surface revision and hook counts unchanged after transitions finish |
| Persistence | Settings/catalog durable; private live state excluded; native private-tab environment applied |
| Hidden/collapsed modals | Native navigator and semantic forms temporarily use expanded space; Escape restores the preference. Actual hidden-navigator GUI run preserves split and background-tab geometry |
| Installed macOS bundle | Strict signature verification; managed launcher in a quoted custom path; bundled SQLite helper; default Rust UI without Lua; native OpenGL/CGL |
| Native tab moves | Local split and remote single-pane moves preserve panes and pins. Remote destination resizing is checked against server geometry; actual close/Reopen remains functional |
| External pane moves | A second Unix mux CLI client moves the pane; GUI ownership converges without a duplicate owner or false Reopen entry, and the moved process survives closing its source window |
| Transport | No GUI or mux transport errors in the completed runs |
| Isolation | Separate fixture executables, configuration, database, workspaces and SSH credentials |

Native sidebar CPU timings below include mixed warm/cold frames from each main window. They exclude GPU completion, display presentation and physical input latency. Samples differ by scenario; these are observations, not portable performance thresholds.

| Session | Paint samples | Compose + convert p50 / p95 / p99 | Native paint p50 / p95 / p99 |
| --- | ---: | ---: | ---: |
| Local | 81 | 17 / 125 / 164 µs | 62 / 641 / 1,342 µs |
| Unix mux | 184 | 10 / 87 / 114 µs | 45 / 511 / 874 µs |
| SSH mux | 143 | 10 / 93 / 113 µs | 46 / 695 / 1,330 µs |

| Cause addressed | Implementation |
| --- | --- |
| Competing size corrections | One native viewport reservation and ordinary WezTerm content resizing |
| Remote resize feedback | Coalesced topology snapshots wait for resize acknowledgements and reject superseded results; accepted snapshots do not echo resize RPCs |
| Delayed sidebar snapping | Immediate bounds and complete staged surfaces; no settling timer |
| Unexpected pane movement | Sidebar has no pane identity; presentation never changes split topology |
| Stale tab geometry | Window-owned reservation; background tabs and new tabs use current content dimensions |
| Repeated composition | Retained native rows and quads; unchanged terminal output reuses the sidebar |

| Remaining validation | Boundary |
| --- | --- |
| Linux/Windows runtime | Build/package workflow provided; those operating systems were unavailable for this implementation session |
| SSH coverage | Real SSH transport on localhost; no Windows GUI or cross-host network-latency run |
| Display behavior | OS drag smoothness, visible flashes, GPU timings and input-to-display latency require capture/profiling |
| Input/rendering | Physical IME, monitor/DPI transitions, fallback fonts and screenshot inspection remain manual checks; Unicode conversion, caret geometry and clipping have deterministic tests |
| macOS capture | Fixture-window capture was unavailable; no visual result is inferred from geometry samples |

Reproduce with the commands in [Development](development.md). Scenario output contains `report.json`, `samples.jsonl` and isolated GUI/mux logs.
