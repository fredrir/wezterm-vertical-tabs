tool := "cargo xtask"

_default:
    @just --list --unsorted

# Build and install; --on MACHINE compiles, --to MACHINE receives, --rollback [ID].
deploy *args:
    @{{tool}} deploy {{args}}

# Compile and validate; --on MACHINE compiles, --to MACHINE bundles for another machine.
build *args:
    @{{tool}} build {{args}}

# Iteration bundle; hot-reloads changes unless --no-watch.
dev *args:
    @{{tool}} dev {{args}}

# Suite: all/tools/rust/lua/gui/ssh/tls/bench; --on MACHINE; pytest arguments follow --.
test *args:
    @{{tool}} test {{args}}

# Formatting, Clippy, Ruff and generated files; --fix rewrites them; --on MACHINE.
lint *args:
    @{{tool}} lint {{args}}

# Active, pending and previous versions; --to MACHINE.
status *args:
    @{{tool}} status {{args}}

# System and Python dependencies; --check only diagnoses; --on MACHINE.
setup *args:
    @{{tool}} setup {{args}}
