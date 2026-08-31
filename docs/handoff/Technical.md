## Technical Context

### Dependencies

| What | Why | How |
| --- | --- | --- |
| none added | PNG encoder hand-rolled in `backend/src/frame.rs` | `wez-vtabs --frame` writes the layer PNG |
| `wezterm cli --no-auto-start` | kill sidebar panes without spawning a mux | invoked from `wezterm.executable_dir`, socket pinned `gui-sock-<pid>` |
| WezTerm sparse source | source-verified mux facts (resize dealing, overlay scope) | `~/.cache/vtabs-team/wezterm-src` |

### Configuration Changes

| What | Why | How |
| --- | --- | --- |
| `frame` | zen frame opt-in | `"zen"` \| off (default) — default-on awaits fredrir |
| `meta` | cwd/socket lines leak paths | default off |
| `strip_actions` | strip buttons declarative | default `{ "toggle", "new_tab", "settings" }` |
| `settings.json` | GUI page writes changed keys only | `util.write_private`, 0600, versioned |
| `VTABS_E2E_MACOS=1`, `VTABS_STRESS_SOFT=1`, `VTABS_DUMP_FRAMES` | harness switches | see `plugin/tests/*.sh` |

### Files Changed

| What | Why |
| --- | --- |
| `plugin/vtabs/mux.lua` | new façade — the one guarded place to touch a WezTerm handle |
| `plugin/vtabs/actions.lua` | single name→behaviour dispatch table (fixed inert ⚙) |
| `plugin/vtabs/layout.lua` | derives strip defaults from `actions.strip`; owns grid/hits |
| `plugin/vtabs/frame.lua` + `backend/src/frame.rs` | zen frame canvas + PNG writer |
| `plugin/vtabs/geometry.lua` | sole resize owner; adoption predicates |
| `plugin/vtabs/{settings,page,schema}.lua` | settings tab, form, single source of truth for options |
| `plugin/tests/run_<module>.lua` + `tests/support/helpers.lua` | 6.9k-line `run.lua` split; `run.lua` now a 40-line runner |
| `scripts/baseline.sh`, `scripts/screenshot.sh` | pin/check gating; 24+ table-driven screenshot states |
