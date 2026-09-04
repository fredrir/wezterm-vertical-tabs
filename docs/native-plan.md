**Native vertical tabs — Rust-first implementation plan**

| Name | Value |
| --- | --- |
| Status | Planned; names and contracts below are proposed |
| Architecture | Project-owned Rust application linked into the patched WezTerm GUI |
| UI | Ratatui widgets and buffers; TachyonFX effects; one custom native adapter |
| Presentation | WezTerm's existing text shaping, glyph caches, quads, and compositor |
| Lua | Optional configuration, styling overrides, and semantic hooks |
| Persistence | Plugin-owned SQLite helper, invoked asynchronously from Rust |
| Upstream | Latest `wezterm/wezterm` `main`, resolved at build time; no revision pin |
| Platforms | macOS, Linux, Windows |
| Sessions | Local, Unix mux where upstream supports it, SSH mux |
| Distribution | One native bundle; the complete Rust UI works with defaults without loading Lua |
| Performance priority | Frame latency, input responsiveness, idle cost, allocations, and mux traffic; measured in the integrated GUI |
| Maintenance priority | Narrow upstream adapter, clear Rust dependencies, one supported implementation |

**1. Use the probe as a CPU baseline**

The 2026-09-05 probe exercised Ratatui composition, TachyonFX effects, buffer diffing, conversion to Termwiz lines, dirty-line copying, and clustering. Three runs measured 72,000 frames on an Apple M5 Pro, macOS arm64, with optimized Rust code.

| Scenario | p95 CPU time across runs |
| --- | ---: |
| Selected-tab change, 32 × 60 cells | 0.109–0.110 ms |
| Animated sidebar, 32 × 60 cells | 0.203–0.205 ms |
| Continuous resize + animation + menu | 0.503–0.508 ms |
| Large animated sidebar, 80 × 160 cells | 1.075–1.092 ms |

| Evidence | Boundary |
| --- | --- |
| Ratatui 0.30.2, TachyonFX 0.25.1, Termwiz 0.23.3 | Observed probe dependencies; production adapter uses the selected upstream checkout's Termwiz |
| Unicode, colors, resize dimensions, overlay replacement | Buffer comparisons passed |
| Staged clear | Previous complete buffer remained published until commit |
| Unchanged redraw | Zero changed cells and dirty rows; widget recomposition still cost CPU |
| Normal animation | TachyonFX averaged about 0.013 ms; diff/conversion/line preparation about 0.115 ms |
| Not measured | Font shaping, glyph-cache invalidation, GPU work, display presentation, native input, mux layout, Lua, SQLite |

Proceed with this architecture. The first implementation milestone must measure the actual native render path; the probe does not establish GUI frame pacing or resolve the reported resize/flash failures by itself.

**2. Separate Rust domains and dependencies**

| Component | Owns | Dependency boundary |
| --- | --- | --- |
| `vtabs-core` | Spaces, routing, pins, ordering, selection policy, settings schema, commands, state transitions | Plain Rust data; no WezTerm, Ratatui, SQLite, filesystem, or subprocess calls |
| `vtabs-ui` | Ratatui composition, interaction state, scrolling, focus, menus, forms, hit regions, TachyonFX | Consumes core view data; emits typed intents; no mux or database access |
| `vtabs-app` | Per-window lifecycle, event reduction, dirty-state scheduling, pending operations, storage requests | Coordinates core/UI through explicit host and storage ports; no private WezTerm types |
| `vtabs-store` | SQLite helper, migrations, reads, transactions, persistence protocol | Stores requested data; never evaluates routing or UI policy |
| `native/adapter/` | Native snapshot/input conversion, host commands, surface conversion, clipboard/IME, registration | Only project component that depends on private WezTerm APIs |
| Lua layer | Configuration, theme overrides, optional callbacks, opt-in feature bindings | Validated updates and semantic events; never frame buffers or per-frame control |
| Native patches | Viewport, tab projection, generic surfaces/input hooks, frame scheduling | No space rules, card presets, settings schema, SQLite, or product effects |

Use modules within these components for smaller concerns. Extract further crates only for real dependency boundaries. The configuration schema belongs to Rust; generate Lua types, settings descriptors, and option documentation from it.

```text
Native event → adapter → Rust application/core → host command, UI invalidation, storage request
Dirty UI → Ratatui buffer → TachyonFX → native surface adapter → WezTerm frame
Storage request → asynchronous SQLite helper → revision-tagged result → Rust application
Lua configuration/hook result → validated semantic update → Rust application
```

Keep UI/application state on the GUI event loop. Avoid a second rendering thread, shared mutable UI state, or locks around every model operation. Slow I/O executes outside that loop and returns bounded, revision-tagged results.

**3. Keep the native integration narrow**

| Artifact | Content |
| --- | --- |
| `native/patches/0001-native-tab-bar.patch` | Left/right viewport reservation, visible-tab projection, native navigation and lifecycle integration |
| `native/patches/0002-native-ui-host.patch` | Generic native line/layer surfaces, clipping/transforms, input/focus/IME hooks, frame deadlines, provider registration |
| `native/adapter/` | Project-owned Rust module implementing the host contract and connecting `vtabs-app` to WezTerm |
| Build integration | Stage the adapter and wire project crate dependencies into the tool-owned checkout; compile everything together |

Apply the patches as one series, independently implemented against upstream source. Put substantial host code in focused new modules. Limit edits to existing files to required layout, paint, input, action-dispatch, and registration call sites; avoid unrelated refactors and mass formatting.

Feature code stays in this project and is linked into the GUI binary. The adapter uses upstream's Termwiz, font, window, and renderer types; do not link a separate published Termwiz renderer into production. Ratatui and TachyonFX remain project UI dependencies.

Enable only required library features. Omit terminal-output/input adapters and the TachyonFX runtime DSL unless an implemented feature needs them; native input and compiled Rust effects are the default.

The host contract handles native surfaces and events. Rust UI modules implement widgets, forms, menus, and focus policy. Use one statically registered provider; no dynamic Rust library loader, runtime ABI framework, rendering process, PTY, or ANSI transport. Ratatui draws through the custom in-memory adapter rather than a terminal-output adapter.

Compiled feature/UI changes require rebuilding the bundle. Lua configuration and styles remain reloadable. Compile-time checks and a small current-contract capability marker replace compatibility branches.

**4. Define atomic native transactions**

| Contract | Required behavior |
| --- | --- |
| Host snapshot | Stable tab IDs/metadata, active tab, viewport/font metrics, revision covering topology, selection, order, geometry, and configuration epoch |
| Application update | Reduce cached Rust state synchronously for native interaction; return explicit host commands and affected UI regions |
| View commit | Publish visible tab order, selected ID or empty selection, layout, surfaces, and hit/focus targets as one coherent revision |
| Stable identities | Tab IDs and element IDs; never store action targets as transient visible indices |
| Stale results | Reject responses superseded by native selection, resize, topology, or configuration changes; coalesce one fresh update |
| Invalid projection | Reject foreign/missing/duplicate IDs and selection outside the visible list |
| Presentation | Native drawing and input consume the committed selection and geometry |
| Concurrent close | Remove invalid targets before painting/input; never activate a different tab through a reused index |
| External activation | Reconcile the actual ID with Rust space policy before presenting mismatched content and sidebar state |
| Async spawn | Reserve current content dimensions at spawn; assign membership and publish the new active view before first presentation |
| Hook/store completion | Return semantic results only; never replace the whole model with an old snapshot |
| Lock lifetime | Do not call hooks, await I/O, or invoke reentrant host actions while holding mux locks |

Projection and geometry commits run on the GUI loop. Ordinary activation uses cached Rust state and does not await a callback, metadata refresh, database request, or UI process. The next native frame reflects the selection.

**5. Implement the render path for low latency**

| Stage | Implementation |
| --- | --- |
| Invalidation | Track model, layout, theme, input, and active-effect changes separately |
| Model access | Index tabs by stable ID; cache visible order and metadata; update affected entries instead of scanning or serializing the full model on every event |
| Native paint with unchanged sidebar | Reuse cached sidebar surfaces; terminal output alone must not trigger Ratatui recomposition |
| Ratatui composition | Render the complete visible surface when dirty; reuse allocated buffers and cached text/layout inputs |
| Lists | Lay out visible cards/rows plus bounded overscan; hidden tabs remain model data |
| Effects | Reuse TachyonFX instances; process active regions after widget composition |
| Diff | Translate changed cells; avoid repeated full-buffer conversion |
| Native rows | Update dirty rows; retain line/cluster/glyph-related caches while their inputs remain valid |
| Color-only effects | Preserve shaping work where upstream's cache contract permits; distinguish color from glyph/font/geometry changes |
| Clear/resize | Mutate staging only; publish complete output with matching dimensions and hit targets |
| Buffer ownership | Reuse and swap ownership; avoid cloning the full grid, metadata, or model every frame |
| Unicode | One explicit UI cell-width policy; preserve widths during conversion; verify graphemes, emoji, CJK, RTL, and fallback fonts |
| Composition | Draw sidebar, overlays, and content in the same WezTerm frame |
| Clock | WezTerm's monotonic frame clock drives effects, caret deadlines, and transitions |
| Idle | No continuous UI timer or render loop; request frames only for invalidation or an active deadline |
| Overload | Coalesce pending visual changes to the newest state; no animation-frame backlog |
| Hidden/unfocused UI | Stop unnecessary effects; reconcile state before the next visible frame |

Profile widget composition, effects, diff/conversion, clustering/shaping, GPU submission, and presentation separately. Optimize the measured dominant stage. Rust and these libraries do not establish a universal performance guarantee.

Use `opt-level = 3` for project release code. Start with upstream-compatible build settings; compare LTO settings only if integrated measurements justify a change. The probe used thin LTO. Keep distributed binaries compatible with their target architecture; avoid machine-specific compiler flags for general releases.

| Animation | Owner and rule |
| --- | --- |
| Text, color, cell effects | TachyonFX, defined in `vtabs-ui` |
| Rail/card/menu behavior | Rust UI policies and settings |
| Whole-surface movement/opacity | Generic native transforms and clipping, driven by Rust UI transition state |
| Sidebar reservation | Commit final width once; visual transitions must not repeatedly resize content |
| Window resize | Immediate native layout; never wait for a quiet period or settling effect |
| Cancellation | Retarget/cancel effects on new input, resize, reload, and teardown |
| Accessibility | Honor disabled/reduced animation settings; interactions remain immediate |

Ratatui supplies cell-based presentation. Smooth movement of a whole surface uses native pixel transforms. Do not promise arbitrary pixel-positioned widget layouts from a cell grid.

**6. Make native geometry and navigation authoritative**

| Layout requirement | Completion condition |
| --- | --- |
| Central calculation | Produce sidebar, content, overlay, and safe-area rectangles once |
| Width and DPI | Reserve logical pixels; convert to UI cells using current font metrics; handle residual pixels natively |
| Left/right | Share the same calculation and coordinate transforms |
| Decorations | Account for padding, borders, title bars, and macOS window buttons exactly once |
| Tiny windows | Clamp against the native minimum terminal area; no correction retries |
| Pixel resize within the same cell grid | Update bounds/clipping; reuse cell content when its layout inputs are unchanged |
| Cell-grid resize | Synchronously rebuild affected visible UI for the latest dimensions before presentation |
| Content consumers | Rendering, pointer-to-cell mapping, selection, cursor/IME, split handles, scrollbars, search/copy overlays, pane selectors, and zoom use the same content rectangle |
| Tab resize | WezTerm's ordinary resize path for actual content; no plugin geometry commands |
| Background tabs | Current content dimensions through normal native window sizing |
| Initialization | Reserve sidebar before the first pane; create subsequent tabs at current content size |
| Split integrity | Resizing/activation never add, classify, move, remove, or rearrange content panes |

Match ordinary WezTerm split sizing and cell rounding for an equivalent content viewport. The sidebar is a window-owned native surface and never part of a tab's split tree.

Use a GUI-local ordered visible-tab projection. Spaces stay in Rust plugin state; filtering does not move tabs into mux windows or WezTerm workspaces.

| Native action/event | Behavior |
| --- | --- |
| `ActivateTab(i)` | Index visible tabs; preserve zero-based and negative-index semantics |
| Relative/no-wrap activation | Traverse the same visible order |
| `ActivateLastTab` | MRU restricted to visible tabs |
| Tab navigator and numbering | Use the visible projection |
| Reorder | Resolve positions to IDs; apply Rust pin/group policy and commit order atomically; preserve hidden tabs |
| Close active tab | Select visible neighbor; if none remains, show the selected space's empty view |
| Close final actual tab | Preserve upstream window-close policy |
| Explicit activation by actual ID | Follow the tab's space in the same presentation transaction |
| Raw mux/CLI enumeration | Preserve actual identities/order; GUI-local indices do not redefine remote addressing |
| Native new tab | Current content size and Rust routing before first presentation |
| Pane navigation, split, zoom | Ordinary native actions on actual content panes |
| Empty view | Keep hidden tabs running; show New tab; suppress hidden terminal drawing/input |
| Spawn from empty view | Retain relevant/default native spawn domain and selected plugin space |
| Multiple GUI clients | Independent presentation state, respecting normal upstream shared-mux focus/resize behavior |

The user's bindings stay valid:

```lua
for i = 1, 9 do
  table.insert(keys, {
    key = tostring(i),
    mods = MOD.PRIMARY,
    action = act.ActivateTab(i - 1),
  })
end
```

Audit every native action path, including tab-bar clicks and keyboard actions. Do not install replacement tab-navigation, pane-navigation, or split bindings. Plugin-specific actions remain opt-in.

**7. Build the complete Rust UI and feature layer**

| Feature | Required result |
| --- | --- |
| Spaces | All spaces in the current profile scope, including empty spaces; names/icons, selection, activity indicators |
| Create space | Always-reachable `(+)`, distinct from New tab; native text entry and SQLite persistence |
| Space overflow | Scrollable entries; keep `(+)` accessible at every supported size |
| Space lifecycle | Create, rename, reorder, edit, delete; select a destination when deleting a nonempty space |
| Window selection | Remember selected space per window; commit projection, content selection, and UI together |
| Empty spaces | Stay selectable; never auto-spawn shells or expose another space's terminal |
| Routing | Declared/dynamic spaces, templates, metadata rules, manual assignment, return-to-auto, optional cached hook results |
| Tab presentation | Cards/rows, metadata, titles, icons, numbering, unread/bell indicators, pins, separators, close controls, tooltips |
| Actions | Native create/activate/close/rename/reorder; Rust close-others, pin, move-to-space, private-window, reopen policies |
| Tab/window moves | Supported native operations on explicit user intent; preserve affected content and split layout |
| Menus | Ratatui surfaces, custom entries, nested menus, confirmation, keyboard operation, cancellation |
| Settings | Rust-schema-driven Ratatui forms, validation, persistence, reset, immediate application |
| Rail | Expanded/collapsed/hidden, actions, safe-area choices, TachyonFX/native surface transitions |
| Themes | Rust defaults, per-space accents, private styling; optional Lua overrides |
| Private state | Keep private live-tab state and reopen history out of persistent storage |

Ratatui handles rendering; `vtabs-ui` must implement interaction. Text input needs grapheme-aware editing, selection, clipboard, IME composition/caret placement, and focus restoration. Menus need hit testing, keyboard traversal, outside-click dismissal, and transformed pointer coordinates. Reuse suitable Rust components or upstream input facilities instead of assuming widgets provide these behaviors automatically.

Use stable IDs for controls and drag targets. UI interaction emits typed intents; domain rules stay in `vtabs-core`. Pane actions go through the adapter. Surface overlays may cover content without changing its layout or acquiring a pane identity.

| Lua/configuration rule | Behavior |
| --- | --- |
| Precedence | Rust defaults → persisted preferences → explicit Lua overrides |
| Schema | Rust authoritative; generate types, settings descriptors, option documentation |
| Config-owned field | Settings identifies the override and does not silently overwrite it |
| Hooks | Optional title/theme/filter/footer/routing callbacks at semantic boundaries; cache results |
| Native interaction | Never await a hook to finish resize/ordinary activation; accepted later results become versioned semantic updates |
| Hook failure/reload | Preserve valid state, report once, discard stale completions |
| Metadata | Native notifications first; bounded refresh only for unavailable notifications; no geometry polling |
| Pane output | Invalidate only on relevant changes such as unread/bell transitions; never rebuild for every output notification |

**8. Keep persistence off the GUI path**

| Name | Value |
| --- | --- |
| Helper | Small Rust binary with bundled SQLite, owned and shipped by the project |
| Caller | Rust application requests storage; adapter schedules work on existing host async/blocking-I/O facilities |
| Execution | Short-lived batched calls; no daemon, rendering loop, or separate async runtime |
| Transport | Versioned bounded requests through piped stdin/stdout; no per-frame serialization/IPC |
| Working state | Rust memory; no database queries to draw or navigate |
| Stored data | Profiles, space catalog/order/rules, settings, window preferences, verified session-scoped assignments/pins |
| Writes | Coalesce durable feature changes; ordinary tab activation remains memory-only |
| Concurrency | Short transactions, busy timeout, field-level operations, revisions for conflicts |
| Backpressure | Bounded queue; merge pending updates instead of accumulating snapshots |
| Failure | Report without blocking UI; retain dirty state for bounded/explicit retry |
| Shutdown | Drain pending durable writes within a bounded shutdown phase; handle failure explicitly |
| Remote sessions | SQLite stays on GUI machine; no remote helper or database synchronization |

The helper owns migrations and transaction execution, with no routing/UI decisions. Invoke it from Rust without Lua or a shell in the transport path. Use the existing host runtime rather than an executor per window.

| State | Restore rule |
| --- | --- |
| Spaces, ordering, names, icons, rules, settings | Durable independently of shell/mux lifetime |
| Live assignments/pins | Restore only within verified session incarnation and tab identity |
| Unverified restart/reconnect | Preserve definitions; reroute discovered tabs instead of matching reused IDs, titles, or cwd |
| Reopen history | Explicit launch metadata where available; does not restore dead-process execution state |

Keep the mux wire protocol unchanged. Restoring live assignments across GUI restarts remains limited by the identity upstream exposes. Verify each domain; do not move persistence or spaces into the native patch.

**9. Follow latest main with small build/update tooling**

| Component | Behavior |
| --- | --- |
| Shared script | One cross-platform refresh/apply/build/check/package/install entry point |
| Checkout | Tool-owned cache/worktree outside project source; fetch `origin/main` and submodules |
| Apply | Clean disposable worktree; ordered patches with `git apply --check`; stage adapter and dependency wiring |
| Revision | Resolve latest `main` for each build; retain SHA only as diagnostic build metadata |
| Development | `just dev`/`just build` refresh automatically and reuse compilation caches; Rust changes rebuild, Lua overrides reload |
| Checks | `just check`: focused Rust core/UI/adapter/storage checks and Lua configuration contract checks |
| CI | One workflow for project changes, daily upstream checks, and manual runs; small macOS/Linux/Windows build matrix |
| Daily build | Resolve current project/upstream heads; skip already-built combinations |
| Bundle | Patched GUI with linked Rust application, SQLite helper, optional Lua, capability/build metadata |
| Packaging | Follow upstream target/toolchain and packaging conventions |
| Updates | Asynchronous launch check at most daily; install completed bundle between launches |
| Manual force | `just update` uses the same update path |
| Running sessions | No replacement of executing binaries or restart of attached sessions during updates |
| Breakage | Report apply/build failure; repair current integration without revision fallback, compatibility branches, or automatic patch rewriting |

Routine upstream monitoring and source syncing are automatic. Incompatible upstream changes still need code fixes; isolate those to the adapter/host integration where possible. Dependency lockfiles are separate from pinning a WezTerm revision.

Only the GUI requires vertical-tabs integration. Compatible upstream mux servers require no feature installation; advancing `main` can require ordinary upstream client/server upgrades.

**10. Validate native behavior and complete one cutover**

| Acceptance check | Required evidence |
| --- | --- |
| Integrated render probe | Ratatui/TachyonFX surfaces actually painted by WezTerm; warm/cold shaping, conversion, GPU work, and presentation measured |
| Frame pacing | p50/p95/p99 CPU and input-to-present latency, missed frames, idle wakeups; compare stock WezTerm from the same checkout with equivalent content viewport |
| CPU reference | Reproduce small probe; use observed costs to detect regressions on the reference machine, not portable hard CI timing thresholds |
| Continuous resize | Grow/shrink/reverse without wrong-width frames, delayed snaps, or post-drag corrections |
| Content splits | Pane IDs and split topology unchanged; distribution matches ordinary native behavior |
| Native activation | Correct visible tab on next available native frame; no width change, UI startup, or full-content flash |
| Native new tab | Correct reserved content size from creation through first presentation |
| Background tabs | Correct geometry on first displayed frame after resize |
| Native actions | Indexed/negative, relative/no-wrap, MRU, navigator, reorder, close successor, direct-ID activation |
| Spaces | All entries and `(+)`; create/select/delete, empty views, routing, pins, native new-tab behavior |
| Geometry/input | Left/right, rail/hidden, tiny windows, font/DPI, monitor move, fullscreen, zoom, overlays, scrollbars, selection, clipboard, IME, pointer transforms |
| Lifecycle races | Close during update, rapid spaces, async spawn, reload, reconnect, teardown, stale hook/store results |
| Animation | Cancellation, retargeting, finite completion, reduced motion; no frames after deadlines/invalidations finish |
| Rendering correctness | Wide-to-narrow replacement, emoji, combining text, RTL, fallback fonts, palettes, exposed pixels after shrink |
| Storage | Transaction failure, concurrent windows, private mode, shutdown writes, reused IDs, reconnect identity |
| Hot-path work | Zero plugin geometry commands, CLI connections, synchronous Lua calls, helper launches, or database waits during resize/ordinary activation |
| Idle/output | Stable sidebar has no periodic work and reuses surfaces during unrelated terminal output |
| Transport errors | Attribute remaining broken pipes to actual connections; investigate independently of UI performance |
| Platforms/domains | Local GUI on all three OSes; Unix mux on supported hosts; SSH mux including a Windows GUI client |

Keep deterministic tests for real invariants and a compact GUI scenario runner. Capture transient geometry/frame behavior rather than only eventual sizes. Perform visual resize/DPI/IME checks during implementation; avoid an extensive always-running GUI test pipeline.

| Reported issue | Structural resolution |
| --- | --- |
| Competing resize updates and excess mux work | One native geometry owner; Rust UI never corrects pane sizes |
| Sidebar snapping | Immediate native bounds/cell-grid handling; no settling strategy |
| Incorrect pane movement | Presentation cannot classify or rearrange content panes |
| Stale geometry on native switching | Window-owned reservation, current background-tab dimensions, atomic commits |
| Flash and latency | Correct initial content size, complete staged surfaces, one native compositor frame |

Complete the full Rust feature/UI layer before the supported release cutover. Remove superseded runtime files, generated artifacts, dependencies, selectors, configuration switches, action wrappers, tests, workflow jobs, and documentation. Keep one supported system and no compatibility aliases or alternate implementation paths.

```text
crates/vtabs-core/    # domain model, reducers, schema
crates/vtabs-ui/      # Ratatui composition, input state, TachyonFX
crates/vtabs-app/     # application lifecycle and ports
crates/vtabs-store/   # SQLite helper and migrations
native/patches/       # ordered generic host patches
native/adapter/       # project-owned WezTerm integration
plugin/              # optional Lua configuration/hooks, generated types
scripts/             # shared build/update/check tooling
tests/               # focused contracts and native GUI scenarios
docs/                # current configuration, development, API reference
.github/workflows/   # one small build workflow
```

| Delivery order | Exit condition |
| --- | --- |
| 1. Actual native adapter | Minimal linked Rust application paints Ratatui/TachyonFX through WezTerm; real frame timing and input/resize measured |
| 2. Geometry and transactions | One viewport owner; correct initial size; coherent staged surfaces and input targets |
| 3. Core and native actions | Rust space/order policy, native shortcuts, lifecycle, empty views, background tabs agree |
| 4. Complete Rust UI | Spaces with `(+)`, cards, menus, text input, settings, rail, themes, feature actions |
| 5. Persistence and Lua | Asynchronous SQLite, verified restore boundaries, Rust-generated schema/types, optional hooks |
| 6. Build/update tooling | Fresh latest-main builds, packaging, automatic updates on target platforms |
| 7. Acceptance and cutover | Native regressions checked, platform/domain coverage complete, one native system |

Track this plan during implementation. On completion, retain the small code-verified API/configuration/build references and remove this planning document so it does not become a second specification.
