# Development

```
backend/   Rust input/render bridge (`wez-vtabs`)
plugin/    Lua plugin (`init.lua` + `vtabs/*`), bootstrap scripts, tests
docs/      protocol + configuration
```

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
lua tests/run.lua          # unit tests: wezterm stub + fake mux (any Lua 5.4+)
luacheck init.lua vtabs tests
stylua --check init.lua vtabs tests
```

## End-to-end

Launches a throwaway WezTerm window, drives it with `wezterm cli` and
synthetic SGR mouse sequences, and checks the rendered sidebar:

```sh
sh plugin/tests/e2e.sh          # local domain
sh plugin/tests/e2e.sh mux      # through a unix multiplexer domain
```

## Local development config

```lua
package.path = "/path/to/wez-vertical-tabs/plugin/?.lua;" .. package.path
local vtabs = dofile "/path/to/wez-vertical-tabs/plugin/init.lua"
vtabs.apply_to_config(config, {
  backend = { path = "/path/to/wez-vertical-tabs/backend/target/release/wez-vtabs" },
  debug = true, -- logs events and hit rows (see `wezterm --config-file ... start` output or the debug overlay)
})
```

## Bootstrap environment

`plugin/vtabs/backend.lua` passes these to `plugin/bin/bootstrap.sh|.ps1`:
`VTABS_TARGET` (Rust triple), `VTABS_REPO`, `VTABS_VERSION` (release tag
without `v`), `VTABS_SRC` (backend crate for the cargo fallback), `VTABS_BUILD`
(`0` disables it), `VTABS_BIN` (explicit binary), `VTABS_USERVAR`. The script
extends `PATH` with `~/.cargo/bin`, `/opt/homebrew/bin` and `/usr/local/bin`
because the GUI process has a minimal PATH, and verifies downloads against the
release's `SHA256SUMS`.

## Releasing

Tag `vX.Y.Z` (matching `plugin/vtabs/version.lua`); `.github/workflows/release.yml`
builds `wez-vtabs-<target>` assets that `plugin/bin/bootstrap.sh` downloads.
