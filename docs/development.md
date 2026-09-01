# Development

## Workspace layout

`backend/` is a Cargo workspace; crate boundaries are enforced (`just lint` runs
`lint-crate-deps.sh`/`lint-boundaries.sh`, a `cargo tree` + grep check, not just a review comment).

| Crate | Responsibility |
| --- | --- |
| `vtabs-protocol` | wire types (`Command`, `Event`, `DoArgs`), `VERSION`, `limits::*`, `bin/gen-lua` |
| `vtabs-core` | `UiState`, sanitize, `geom::strip_geometry`, `icons`, `lua_pattern` |
| `vtabs-theme` | palette resolve (`resolve()`), WCAG contrast, mix/luminance |
| `vtabs-view` | ratatui-free widgets and layout: `enrich`, `frame`, `fx`, `glyphs`, `layout`, `menu`, `render`, `scene`, `settings`, `text` |
| `vtabs-input` | `parser` (stdin byte demux) + `resolve` (regions/event/`UiState` → `Event`s), pure |
| `vtabs-runtime` | event loop (`app`), terminal guard, uservar emission, logger, signal handling, paint — the sole stdout writer |
| `vtabs-zen` | `png` + `frame`, the `wez-vtabs frame` subcommand; zero external dependencies |
| `wez-vtabs` (bin) | subcommand dispatch: `frame` \| `dump-frames` \| the runtime |

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

`plugin/vtabs/gen/protocol.lua` is generated from `backend/crates/vtabs-protocol/src/limits.rs` —
the one home for every bound both sides must agree on. Do not hand-edit the generated file.

```sh
just gen-protocol                 # regenerate plugin/vtabs/gen/protocol.lua
cargo run -q -p vtabs-protocol --bin gen-lua -- --check   # fail when stale (run by `just lint`)
```

## The golden oracle

`plugin/tests/golden/scenes/*.json` are committed `RenderInput` scenes; `plugin/tests/golden/frames/`
holds the byte-exact reference renders — a text dump (`<scene>.txt`, column-ruled, ANSI stripped)
and a styled dump (`<scene>.styled.txt`, one `#fg/#bg[*]:width` span per run of cells). `wez-vtabs
dump-frames <scenes-dir> <out-dir>` renders every scene through `vtabs-view::render::golden_dumps`
and writes both dumps per scene.

```sh
cd backend && cargo test -p vtabs-view   # golden_parity.rs + settings_parity.rs byte-diff every scene
just check                               # test + lint — everything CI runs
```

Re-pin only after a reviewed, intentional visual change: `dump-frames` into `plugin/tests/golden/frames`,
then `git diff` and commit.

## Lints

```sh
just lint
```

| Script | Checks |
| --- | --- |
| `scripts/lint-boundaries.sh` | no ANSI escape bytes outside `vtabs-runtime`/`vtabs-input`/ratatui |
| `scripts/lint-crate-deps.sh` | crate dependency direction (§1.1 of the architecture plan), via `cargo tree` |

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