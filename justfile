python := if os() == "windows" { "py -3" } else { "python3" }

_default:
    @just --list --unsorted

# Fetch latest WezTerm main, apply the patch series, build the native GUI.
build *args:
    @{{python}} scripts/native.py build {{args}}

# Build and start a separate native GUI process.
dev *args:
    @{{python}} scripts/native.py dev {{args}}

# Focused Rust and native integration contracts; no GUI is launched.
check:
    @{{python}} scripts/native.py check

test:
    cargo test --workspace --all-features --locked
    @{{python}} -m unittest discover -s tests/native -p "test_*.py"

package *args:
    @{{python}} scripts/native.py package {{args}}

# Install immutable versions; existing GUI processes keep their binaries.
install *args:
    @{{python}} scripts/native.py install {{args}}

# Build latest main and select the completed bundle for subsequent launches.
update *args:
    @{{python}} scripts/native.py update {{args}}

launch *args:
    @{{python}} scripts/native.py launch {{args}}

doctor:
    @{{python}} scripts/native.py doctor

generate:
    cargo run --quiet --locked -p vtabs-core --bin gen-schema -- --write lua plugin/schema.lua
    cargo run --quiet --locked -p vtabs-core --bin gen-schema -- --write types plugin/types.lua
    cargo run --quiet --locked -p vtabs-core --bin gen-schema -- --write markdown docs/options.md
