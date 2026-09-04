_default:
    @just --list --unsorted

# Add --mux to run the sandbox GUI against an owned standalone mux domain.
dev verbose="" *args:
    @sh scripts/dev.sh {{verbose}} {{args}}

deploy *args: # --from-dev (default) / --from-prd / --from-release
    @sh scripts/deploy.sh {{args}}

doctor:
    @sh scripts/doctor.sh

# Restart the GUI and disposable vtabs panes; use --mux only as a last resort.
restart *args:
    @sh scripts/restart.sh {{args}}

# Everything CI runs
check: test lint

test:
    sh tests/test_dev_monitor.sh
    sh tests/test_restart.sh
    cd backend && cargo test --locked
    @just test-plugin
    @just test-tui

# Fast headless tests against production Lua modules in fresh Lua processes
test-plugin *pytest_args:
    uv sync --locked
    uv run --frozen pytest -q -m plugin tests/plugin {{pytest_args}}

# Fast black-box tests against a fresh wez-vtabs PTY per test
test-tui *pytest_args:
    cd backend && cargo build --locked --quiet -p wez-vtabs
    uv sync --locked
    WEZ_VTABS_BIN="{{ justfile_directory() }}/backend/target/debug/wez-vtabs" uv run --frozen pytest -q -m tui tests/tui {{pytest_args}}

# Full real-GUI suite against a fixture-owned standalone mux domain
test-wezterm-e2e *pytest_args:
    @just _test-wezterm-e2e "not ssh_mux_e2e" {{pytest_args}}

# Representative subset of the real standalone mux suite
test-wezterm-e2e-smoke *pytest_args:
    @just _test-wezterm-e2e "smoke and not ssh_mux_e2e" {{pytest_args}}

_test-wezterm-e2e markers *pytest_args:
    @command -v wezterm >/dev/null || { echo "wezterm must be installed and available on PATH" >&2; exit 127; }
    cd backend && cargo build --locked --quiet -p wez-vtabs
    uv sync --locked
    VTABS_E2E_WORKERS="${VTABS_E2E_WORKERS:-2}"; WEZ_VTABS_BIN="{{ justfile_directory() }}/backend/target/debug/wez-vtabs" uv run --frozen pytest -q -n "$VTABS_E2E_WORKERS" -m "{{markers}}" tests/wezterm {{pytest_args}}

# Linux-only, containerized localhost SSH mux-domain contract
test-wezterm-ssh-e2e *pytest_args:
    @command -v wezterm >/dev/null || { echo "wezterm must be installed and available on PATH" >&2; exit 127; }
    @command -v docker >/dev/null || { echo "docker must be installed and available on PATH" >&2; exit 127; }
    @test "$(uname -s)" = Linux || { echo "the SSH e2e lane requires Linux" >&2; exit 1; }
    cd backend && cargo build --locked --quiet -p wez-vtabs
    uv sync --locked
    VTABS_SSH_E2E=1 WEZ_VTABS_BIN="{{ justfile_directory() }}/backend/target/debug/wez-vtabs" uv run --frozen pytest -q -n 1 -m ssh_mux_e2e tests/wezterm {{pytest_args}}

lint:
    uv sync --locked
    uv run --frozen ruff check tests
    uv run --frozen ruff format --check tests
    cd backend && cargo fmt --check && cargo clippy --all-targets --locked -- -D warnings
    cd backend && cargo run -q -p vtabs-protocol --bin gen-lua -- --check
    cd backend && cargo run -q -p vtabs-engine --bin gen-config -- --check
    cd plugin && luacheck init.lua vtabs types ../tests/wezterm/config.lua ../tests/typecheck/fixtures ../tests/plugin/lua/wezterm_stub.lua && stylua --config-path stylua.toml --check init.lua vtabs types ../tests/wezterm/config.lua ../tests/typecheck/fixtures ../tests/plugin/lua/wezterm_stub.lua
    lua scripts/gen-docs.lua --check

# Check the public LuaCATS contract with lua-language-server
typecheck:
    uv sync --locked
    uv run --frozen pytest -q -m typecheck tests/typecheck

# Regenerate all Rust-owned Lua mirrors and their derived documentation
generate: gen-protocol gen-config docs

# Regenerate plugin/vtabs/gen/protocol.lua from vtabs-protocol/src/limits.rs
gen-protocol:
    cd backend && cargo run -q -p vtabs-protocol --bin gen-lua

# Regenerate runtime and LuaCATS schemas from vtabs-engine's typed descriptors
gen-config:
    cd backend && cargo run -q -p vtabs-engine --bin gen-config

# Regenerate the options table in docs/configuration.md from the generated schema
docs:
    lua scripts/gen-docs.lua

build profile="release":
    cd backend && cargo build --locked {{ if profile == "release" { "--release" } else { "" } }}

tag:
    @sh scripts/tag.sh

ls:
    @sh scripts/ls.sh
