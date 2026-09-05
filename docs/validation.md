# Native validation

| Name | Value |
| --- | --- |
| Date | 2026-09-05 |
| Runtime host | Linux x86_64, AMD Ryzen 7 9800X3D, release builds |
| Display | Isolated Xvfb display with Openbox and software rendering |
| Upstream tested | `e019f1b11e9902a771aabc1a8661cc59731b95c0`; builds resolve latest main |
| Renderer | Actual WezTerm GUI, retained Ratatui text, native rounded geometry and TachyonFX |
| Project checks | Rust tests, format, Clippy, generated Lua/schema/docs checks and Python tooling tests |
| Native checks | GUI adapter/host, mux topology races and platform decoration defaults |

| Session | Resize steps | GUI state samples | Result |
| --- | ---: | ---: | --- |
| Local | 12 | 270 | Passed |
| Unix mux | 12 | 271 | Passed |
| SSH mux | 12 | 272 | Passed; isolated localhost SSH server and keys |
| TLS mux | 12 | 274 | Passed; certificate and hostname verification, mutual authentication |

| Check | Result |
| --- | --- |
| Resize/reversal | Sidebar reservation and foreground/background tab sizes remain coherent |
| Split integrity | Pane identities and topology preserved throughout resize and sidebar visibility changes |
| Spaces | Empty space, native new tab, assignment and workspace out/back with retained selection and pins |
| Layout edges | Left/right, expanded/collapsed/hidden rail, integrated chrome and tiny windows |
| Native presentation | Font-size, fullscreen and pane-zoom round trips passed in all four session types |
| Hooks/effects | All five semantic hooks and finite transitions enabled; idle surface revision and hook counts stabilize |
| Persistence | Settings/catalog durable, concurrent edits merged, private live state excluded and private-tab environment applied |
| Local tab moves | Whole split tree, workspace and pin preserved; native close/Reopen remains functional |
| External Unix moves | Second mux client moves a pane; GUI ownership converges without duplicate ownership or false Reopen entry |
| Moved process | Destination geometry updates and process survives closing the source window |
| TLS authentication | Trusted client succeeds; wrong hostname, untrusted CA and missing client certificate fail |
| Isolation | Separate executables, configuration, database, workspaces, display and transport credentials |

| Physical UI fixture | Result |
| --- | --- |
| Keyboard | New tab, folder, settings, search, sidebar toggle, Escape and editor submission passed |
| Mouse | New tab, activation, folder expand/collapse, tab reorder, tab-to-folder drag, settings and tooltip hover passed |
| Terminal input | No UI shortcut leakage; ordinary shell input and Ctrl+C preserved |
| Settings page | Sidebar remains available; opening and closing preserve terminal and split geometry |
| Screenshots | Sidebar, settings, filtered settings, search and tooltip captured from the actual GUI |
| Focus/editor regressions | Selection, clipboard, grapheme boundaries, IME geometry, keyboard button activation and overlay dismissal covered by deterministic tests |
| Rendering regressions | Rounded border rings, retained text, centered controls and atlas-generation cache invalidation covered by native tests |

Observed GUI CPU time during the 12-step resize sequence is below. Each sample represents a distinct native paint count; startup and subsequent feature scenarios are excluded. These measurements include software rendering work on the fixture host, exclude GPU completion and display presentation, and are not input-latency or portable performance guarantees.

| Session | Paint samples | GUI frame CPU p50 | GUI frame CPU p95 |
| --- | ---: | ---: | ---: |
| Local | 23 | 584 µs | 988 µs |
| Unix mux | 24 | 600 µs | 889 µs |
| SSH mux | 24 | 585 µs | 811 µs |
| TLS mux | 24 | 570 µs | 1,014 µs |

| Implementation | Value |
| --- | --- |
| Viewport | One window-owned native reservation; sidebar has no pane identity |
| Remote resize | Coalesced topology snapshots wait for acknowledgements and reject superseded results |
| Composition | Retained rows and quads; unchanged output reuses the sidebar |
| Primitive cache | Shape generation participates in cache invalidation after atlas rebuilds |
| Scheduling | Finite animation deadlines; storage work remains independent of focused-window painting |
| Storage | Asynchronous bounded helper protocol; no SQLite work in resize or activation paths |

| Verification boundary | Value |
| --- | --- |
| macOS/Windows runtime | Cross-platform build workflow provided; native interaction and screenshots in this run were collected on Linux |
| macOS controls | Platform decoration defaults and header reservations tested; physical traffic-button hit targets require a macOS session |
| SSH/TLS coverage | Real localhost transports; cross-host latency and disconnection behavior require network profiling |
| Display behavior | OS drag smoothness, GPU frame pacing and input-to-display latency require native capture/profiling |
| Input/rendering | Physical IME, monitor/DPI transitions and platform font fallback require device checks |
| Live restoration | Folder catalogs and settings persist; live tab membership needs a verified mux incarnation unavailable in the current upstream integration |
| Remote split moves | Unsupported by upstream's pane-move API; remote single-pane moves supported |

Reproduce with [Development](development.md). Fixtures produce `report.json`, sampled geometry and isolated GUI/mux logs. The physical UI fixture also produces screenshots.
