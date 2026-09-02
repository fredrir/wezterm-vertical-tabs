_default:
    @just --list --unsorted

dev *args:
    @sh scripts/dev.sh {{args}}

deploy *args: # --from-dev (default) / --from-prd / --from-release
    @sh scripts/deploy.sh {{args}}

doctor:
    @sh scripts/doctor.sh

# Tear down every WezTerm GUI and mux pane, restart the launchd mux, then open a fresh GUI.
restart:
    @sh scripts/restart.sh

# Everything CI runs
check: test lint

test:
    cd backend && cargo test --locked
    cd plugin && lua tests/run.lua

lint:
    cd backend && cargo fmt --check && cargo clippy --all-targets --locked -- -D warnings
    cd backend && cargo run -q -p vtabs-protocol --bin gen-lua -- --check
    cd plugin && luacheck init.lua vtabs tests && stylua --check init.lua vtabs tests
    lua scripts/gen-docs.lua --check
    sh scripts/lint-boundaries.sh
    sh scripts/lint-crate-deps.sh

# Regenerate plugin/vtabs/gen/protocol.lua from vtabs-protocol/src/limits.rs
gen-protocol:
    cd backend && cargo run -q -p vtabs-protocol --bin gen-lua

# Regenerate the options table in docs/configuration.md from plugin/vtabs/schema.lua
docs:
    lua scripts/gen-docs.lua

build profile="release":
    cd backend && cargo build --locked {{ if profile == "release" { "--release" } else { "" } }}
