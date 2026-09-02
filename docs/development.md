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
| `wez-vtabs` (bin) | subcommand dispatch: `frame` \| `dump-frames` \| the runtime                                                                |

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

## Protocol mirror

```sh
just gen-protocol
cargo run -q -p vtabs-protocol --bin gen-lua -- --check
```

## `just check`


```sh
cd backend && cargo test -p vtabs-view   
just check
```

Re-pin only after a reviewed, intentional visual change: `dump-frames` into `plugin/tests/golden/frames`,
then `git diff` and commit.

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
just deploy --from-release
```

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
