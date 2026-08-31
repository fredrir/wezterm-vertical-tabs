_default:
    @just --list --unsorted

dev *args:
    @sh scripts/dev.sh {{args}}

deploy *args: # --from-dev (default) / --from-prd / --from-release
    @sh scripts/deploy.sh {{args}}

doctor:
    @sh scripts/doctor.sh

# Everything CI runs
check: test lint frames

test:
    cd backend && cargo test --locked
    cd plugin && lua tests/run.lua

lint:
    cd backend && cargo fmt --check && cargo clippy --all-targets --locked -- -D warnings
    cd plugin && luacheck init.lua vtabs tests && stylua --check init.lua vtabs tests
    lua scripts/gen-docs.lua --check

# Re-renders every golden scene and byte-compares text + styled dumps; no display, no backend build
frames:
    @sh scripts/check-frames.sh

# Regenerate the options table in docs/configuration.md from plugin/vtabs/schema.lua
docs:
    lua scripts/gen-docs.lua

e2e mode="local": build
    sh plugin/tests/e2e.sh {{mode}}

build profile="release":
    cd backend && cargo build --locked {{ if profile == "release" { "--release" } else { "" } }}

screenshot state="":
    xvfb-run -a -s "-screen 0 1600x900x24" sh scripts/screenshot.sh {{state}}

baseline *args:
    @sh scripts/baseline.sh {{args}}

baseline-check:
    @sh scripts/baseline.sh --check
