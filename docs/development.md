# Development

## Backend

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

## Options

`plugin/vtabs/schema.lua` is the single source of truth: `config.defaults`, validation and the
options table in `configuration.md` all derive from it.

```sh
just docs                       # regenerate the options table
lua scripts/gen-docs.lua --check  # fail when it is stale (run by `just lint`)
```

| descriptor field | meaning |
| ---------------- | -------- |
| `key` | dotted path, e.g. `padding.top`, `backend.repo` |
| `type` | `number` `string` `boolean` `enum` `table` `function` `any` |
| `default` | omitted when the option has none |
| `enum` `min` `max` | validation; `alias` maps values users already write onto the canonical one |
| `container` `open` | `open` containers (`theme`, `icon_map`, `keys`, `private.env`) do not enumerate their children |
| `docs` | `false` hides the row from the generated table |
| `shown` | overrides the rendered default cell, backticks included |
| `label` `group` `help` | settings UI and docs text |

## End-to-end

```sh
sh plugin/tests/e2e.sh          # local domain
sh plugin/tests/e2e.sh mux      # through a unix multiplexer domain
```

## Dev loop

Needs [`just`](https://github.com/casey/just) and [`watchexec`](https://github.com/watchexec/watchexec).

```sh
just              # list recipes
just dev          # sandbox WezTerm, rebuild + hot-swap on change
just dev --live   # hot-swap the sidebars in your running WezTerm instead
just doctor       # which backend is running, and whether the installs agree
```

## Applying a build

```sh
just deploy                  # build release, hot-swap your running sidebars
just deploy --from-prd       # install into WezTerm's plugin dir as a real plugin
just deploy --from-release   # download the published assets and install those
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