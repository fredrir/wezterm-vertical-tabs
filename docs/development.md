# Development

## Workspace layout


| Crate             | Responsibility                                                                                                              |
| ----------------- | --------------------------------------------------------------------------------------------------------------------------- |
| `vtabs-protocol`  | wire types (`Command`, `Event`, `DoArgs`), `VERSION`, `limits::*`, `bin/gen-lua`                                            |
| `vtabs-core`      | `UiState`, sanitize, `geom::strip_geometry`, `icons`, `lua_pattern`                                                         |
| `vtabs-theme`     | palette resolve (`resolve()`), WCAG contrast, mix/luminance                                                                 |
| `vtabs-view`      | ratatui-free widgets and layout: `enrich`, `frame`, `fx`, `glyphs`, `layout`, `menu`, `render`, `scene`, `settings`, `text` |
| `vtabs-input`     | `parser` (stdin byte demux) + `resolve` (regions/event/`UiState` → `Event`s), pure                                          |
| `vtabs-runtime`   | event loop (`app`), terminal guard, uservar emission, logger, signal handling, paint — the sole stdout writer               |
| `vtabs-zen`       | `png` + `frame`, the `wez-vtabs frame` subcommand; zero external dependencies                                               |
| `wez-vtabs` (bin) | `frame` subcommand dispatch and the runtime entrypoint                                                                      |

```sh
cd backend
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
cargo build --release      # target/release/wez-vtabs
```

## Plugin

```sh
cd plugin
lua tests/run.lua
luacheck init.lua vtabs tests
stylua --check init.lua vtabs tests
```

## Black-box TUI tests

The Python suite drives the real `wez-vtabs` binary through an isolated pseudo-terminal. Install
[`uv`](https://docs.astral.sh/uv/) and run:

```sh
just test-tui
just test-tui -k resize          # focused local run
```

`just test-tui` builds the debug `wez-vtabs` executable once, installs exactly the dependencies in
`uv.lock`, and gives that executable to pytest through `WEZ_VTABS_BIN`. Each test still launches a
new process and PTY session, so mutable terminal state cannot leak between tests. A direct pytest
run may omit `WEZ_VTABS_BIN`; the session fixture then performs one fallback Cargo build for the
whole run.

These tests protect behavior at the terminal boundary: public OSC user-variable events, caller-
supplied content appearing or disappearing, meaningful interaction results, resize/reflow, input
recovery, bounds handling, and clean shutdown. They deliberately do not pin borders, margins,
spacing, coordinates, built-in button labels, or whole-screen snapshots. A visual refactor should
only require a test change when it changes an actual user-facing contract.

Keep scenarios short and outcome-driven. Use fixed terminal sizes and explicit waits, never sleeps,
network access, wall-clock assertions, or shared sessions. The full suite is part of `just test`;
CI runs it on Linux in the existing Rust job and runs only the representative `smoke` subset on
macOS to keep CPU and runner use modest.

For the deliberately small, real-GUI smoke test, install WezTerm and run this from a macOS desktop
session or a Linux graphical session (`DISPLAY` or `WAYLAND_DISPLAY` must be available):

```sh
just test-wezterm-e2e
```

That explicit command sets the suite's opt-in gate, builds the backend once, and starts an isolated
WezTerm instance using `plugin/tests/wezterm-e2e.lua`. It is not a normal PR gate: hosted GUI setup
would add a comparatively slow package download and display-server dependency to a fast PTY suite.
See `tests/wezterm/README.md` for the isolation and assertion policy.

## Protocol mirror

```sh
just gen-protocol
cargo run -q -p vtabs-protocol --bin gen-lua -- --check
```

## `just check`

```sh
just check
```

## Lints

```sh
just lint
```

## Options

```sh
just docs                       # regenerate the options table
lua scripts/gen-docs.lua --check  # fail when it is stale (run by `just lint`)
```

| descriptor field       | meaning                                                                                        |
| ---------------------- | ---------------------------------------------------------------------------------------------- |
| `key`                  | dotted path, e.g. `padding.top`, `backend.repo`                                                |
| `type`                 | `number` `string` `boolean` `enum` `table` `function` `any`                                    |
| `default`              | omitted when the option has none                                                               |
| `enum` `min` `max`     | validation; `alias` maps values users already write onto the canonical one                     |
| `container` `open`     | `open` containers (`theme`, `icon_map`, `keys`, `private.env`) do not enumerate their children |
| `docs`                 | `false` hides the row from the generated table                                                 |
| `shown`                | overrides the rendered default cell, backticks included                                        |
| `label` `group` `help` | settings UI and docs text                                                                      |

## Dev loop

Needs [`just`](https://github.com/casey/just) and [`watchexec`](https://github.com/watchexec/watchexec).

```sh
just    
just dev          
just dev --live
just doctor    
just restart   
```


## Handlers are coroutines



| Fact                                                                                                           | Probe line                                                                  |
| -------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------- |
| every `wezterm.on` emission, `call_after` and `action_callback` is its own Lua thread, `main=false`, yieldable | distinct `thread:` per `status`, `A#n`, `B#n`, `callback`                   |
| `wezterm.sleep_ms` yields to other handlers                                                                    | `B#3`..`B#7` land between `sleep start` and `sleep end`                     |
| a thread keeps its identity across its own awaits                                                              | `split before`/`split after`, `sleep start`/`sleep end` share one `thread:` |
| finished threads are recycled                                                                                  | a `thread:` value reappears on a later, unrelated emission                  |

```sh
HOME=$(mktemp -d) WEZTERM_LOG=info wezterm --config-file scripts/probe-coroutines.lua start --always-new-process 2>&1 | grep probe
```

## Crash bisect

| Trigger                         | Where                                                               |
| ------------------------------- | ------------------------------------------------------------------- |
| `top_level = true` split        | `sidebar_attach.lua` `attach`                                       |
| close by activation             | `sidebar_attach.lua` `close_pane_by_activation`, `close_orphan`     |
| `AdjustPaneSize` by activation  | `geometry.lua` `correct`                                            |
| `set_config_overrides`          | `view.lua` `apply_titlebar_band`, `frame.lua` `install`, `page.lua` |
| `move_to_new_window`            | `actions.lua` `tear_off`                                            |
| `set_inner_size`                | `sidebar_attach.lua` `fit_to_window`                                |
| `cli split-pane --move-pane-id` | `sidebar_rescue.lua` `rescue_splits`                                |

## Applying a build

```sh
just deploy                  
just deploy --from-prd
just deploy --from-release    # newest published GitHub release
```

`just tag` merges `dev` into `main`, creates the next patch release, then fast-forwards and pushes
`dev` to the same release commit. This keeps both branches' version metadata in sync.

## Local development config

```lua
package.path = "/path/to/wezterm-vertical-tabs/plugin/?.lua;" .. package.path
local vtabs = dofile "/path/to/wezterm-vertical-tabs/plugin/init.lua"
vtabs.apply_to_config(config, {
  backend = { path = "/path/to/wezterm-vertical-tabs/backend/target/release/wez-vtabs" },
  debug = true, 
})
```

## Bootstrap environment

| Variable        | Description                          |
| --------------- | ------------------------------------ |
| `VTABS_TARGET`  | Rust triple                          |
| `VTABS_REPO`    | Repository URL                       |
| `VTABS_VERSION` | Release tag without `v`              |
| `VTABS_SRC`     | Backend crate for the cargo fallback |
| `VTABS_BUILD`   | `0` disables it                      |
| `VTABS_BIN`     | Explicit binary                      |
| `VTABS_USERVAR` | User variable                        |
