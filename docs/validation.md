# Validation

| Check                                | Command                                                                                            |
| ------------------------------------ | -------------------------------------------------------------------------------------------------- |
| Project checks                       | `just check`                                                                                       |
| Fast behavior suite                  | `uv run --locked pytest -n 2`                                                                      |
| LuaCATS contracts                    | `uv run --locked pytest -n 2 --run-luals`                                                          |
| GUI, PTY and transport integration   | `uv run --locked pytest -n 2 --run-gui --run-container --run-luals --wezterm-bin-dir=/path/to/bin` |
| Host regressions                     | `just build`                                                                                       |
| Extended geometry and physical input | [Development](development.md)                                                                      |

| Boundary            | Verified behavior                                                                                                                   |
| ------------------- | ----------------------------------------------------------------------------------------------------------------------------------- |
| Rust processes      | Generated schema, bounded storage errors, atomic conflict rollback, concurrent writers, private state and mux incarnation isolation |
| Lua plugin          | Headless configuration, capability failures, options, semantic hooks and action dispatch through the production entrypoint          |
| Lua Language Server | Valid public options accepted; invalid option types and choices diagnosed                                                           |
| PTY                 | Real management CLI and installed `wez-vtabs` launcher output and exit status                                                       |
| Startup             | Local and Unix mux render content; attached remote tabs use the current viewport without a physical resize                          |
| Shutdown            | Owned application quits through its public action and exits successfully before fixture cleanup                                     |
| SSH mux             | Containerized loopback transport, temporary keys, rejected unknown key, new/close/reopen lifecycle                                  |
| TLS mux             | Certificate and hostname verification, mutual authentication, tab lifecycle and key cleanup                                         |
| Geometry            | Resizing, left/right sidebar, expanded/collapsed/hidden rail, integrated chrome, tiny windows, fonts, zoom and fullscreen           |
| Splits              | Pane identities and topology retained; active and background tabs remain sized consistently                                         |
| Mouse               | Tab activation, folder expand/collapse, tab reorder and folder assignment                                                           |
| Keyboard            | Settings, search, tab creation, folders, sidebar toggle, Escape, field editing and terminal Control keys                            |
| Clipboard           | Real OS copy/paste in tab search, settings search and color editor; scoped context menus preserve drafts                            |
| Transient surfaces  | Anchored launcher, menus and tooltips, centered editors, stable sidebar geometry, retained terminal pixels outside popovers         |
| Rendering           | Rounded frame on all four content edges, compact search, centered icon hover, atlas invalidation and row-cache reuse                |
| Scheduling          | Idle composition and semantic hook counts stabilize; animations have finite deadlines                                               |

| Isolation         | Value                                                                                     |
| ----------------- | ----------------------------------------------------------------------------------------- |
| Display           | Owned Linux Xvfb and Openbox; inherited desktop and session endpoints removed             |
| Application state | Separate configuration, database, workspace, executable copies and runtime directories    |
| Processes         | Owned process groups and containers; cleanup covers early exits and failures              |
| Default suite     | No desktop, GPU, WezTerm build, container or external server                              |
| Parallelism       | Two workers by default; shared Rust build cache with locking; separate mutable test state |
| Artifacts         | Temporary JSON reports, sampled geometry, screenshots and application/transport logs      |

| Performance boundary | Value                                                                                                                            |
| -------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| Composition          | Retained rows and geometry; unchanged output reuses cached shapes                                                                |
| Resize               | Coalesced remote topology updates; pixel-origin changes retain text shaping                                                      |
| Storage              | Asynchronous bounded helper protocol; no SQLite work in resize or activation                                                     |
| CPU samples          | Fixture reports cover host work and software rendering; exclude GPU completion and physical presentation                         |
| Device checks        | Physical IME, monitor transitions, macOS traffic controls and input-to-display latency need device profiling                     |
| Platforms            | Build workflow checks macOS, Linux and Windows; automated physical UI tests run on isolated Linux displays                       |
| Restoration          | Durable catalogs/settings; live membership requires a verified mux incarnation unavailable from the current upstream integration |
| Remote moves         | Remote single-pane moves supported; remote split moves remain limited by upstream APIs                                           |
