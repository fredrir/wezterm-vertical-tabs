tool := "cargo xtask"

_default:
    @just --list --unsorted

deps *args:
    @{{tool}} deps {{args}}

# Resolve upstream once, compile and validate native binaries.
build *args:
    @{{tool}} build {{args}}

# Build an iteration bundle; --watch rebuilds changed runtime inputs.
dev *args:
    @{{tool}} dev {{args}}

check *args:
    @{{tool}} check {{args}}

# Select all/tools/rust/lua/native/ssh/tls; pytest arguments follow --.
test *args:
    @{{tool}} test {{args}}

lint:
    uv run --locked ruff check scripts tests
    uv run --locked ruff format --check scripts tests

package *args:
    @{{tool}} package {{args}}

install *args:
    @{{tool}} install {{args}}

update *args:
    @{{tool}} update {{args}}

launch *args:
    @{{tool}} launch {{args}}

doctor *args:
    @{{tool}} doctor {{args}}

generate *args:
    @{{tool}} generate {{args}}

plan *args:
    @{{tool}} plan {{args}}

status:
    @{{tool}} status

versions:
    @{{tool}} versions

rollback *args:
    @{{tool}} rollback {{args}}

cache *args:
    @{{tool}} cache {{args}}

patch *args:
    @{{tool}} patch {{args}}

repro *args:
    @{{tool}} repro {{args}}
