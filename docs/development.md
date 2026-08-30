# Development

```
backend/   Rust input/render bridge (`wez-vtabs`)
plugin/    Lua plugin (`init.lua` + `vtabs/*`), bootstrap scripts, tests
docs/      protocol + configuration
```

## Backend

```sh
cd backend
cargo test
cargo clippy -- -D warnings
cargo build --release      # target/release/wez-vtabs
```

## Plugin

```sh
cd plugin
./lua tests/run.lua        # unit tests against a wezterm stub
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

## Releasing

Tag `vX.Y.Z` (matching `plugin/vtabs/version.lua`); `.github/workflows/release.yml`
builds `wez-vtabs-<target>` assets that `plugin/bin/bootstrap.sh` downloads.
