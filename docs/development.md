# Development

| Name        | Value                                                                                          |
| ----------- | ---------------------------------------------------------------------------------------------- |
| Toolchain   | Stable Rust, Git and platform C/C++ tools; uv/Python 3.12+ for tests |
| Upstream    | `main` resolved once; `--upstream SHA` pins; `dev` reuses the cached revision                                       |
| GUI         | WezTerm renderer with the native patch series and project Rust application                     |
| UI          | Retained Ratatui text, native rounded geometry and finite TachyonFX effects                    |
| Persistence | `wez-vtabs-store`; bundled SQLite, asynchronous bounded JSON requests                          |
| Lua         | Optional configuration, generated schema/types, semantic hooks                                 |

```sh
just check
just build
just dev
just package
just install
just launch
just update
just doctor
```

Recipes invoke `cargo xtask`. Installed launch entries invoke the bundled Rust binary directly.

| Command | Value |
| --- | --- |
| `just deps --upstream SHA` | Run selected upstream system dependency installer |
| `just build --upstream SHA --timings` | Exact upstream, Cargo freshness, native validation, command timings |
| `just dev` | Cached upstream, incremental `iterate` profile, runtime bundle without archive |
| `just dev --watch` | Debounced Rust/adapter/plugin changes; separate owned GUI process |
| `just check` | Rust format/tests/Clippy, schema contracts, Ruff and pytest |
| `just test tools -- -k install` | Focused pytest suite; extra arguments after `--` |
| `just generate` | Generate Lua schema/types and option documentation |
| `just generate --check` | Verify generated artifacts |
| `just package` | Verified bundle, ZIP/tar.gz archive and release manifest in `dist/` |
| `just package --bundle PATH` | Verify and archive an existing bundle |
| `just install --bundle PATH` | Verify and install an immutable local bundle |
| `just launch -- start --always-new-process` | Promote completed pending version and forward GUI arguments |
| `just update --check` | Resolve update availability without compiling or installing |
| `just update --manifest PATH_OR_HTTPS_URL` | Download/copy a verified prebuilt release |
| `just status` / `just versions` | Active, pending, previous and installed versions |
| `just rollback [ID]` | Select a verified installed version; default previous |
| `just plan dev --json` | Inputs and execution decisions without fetch/build |
| `just doctor --for check` | Required tool versions and state health |
| `just patch check --upstream SHA` | Check ordered patches in an isolated worktree |
| `just cache inspect` | Owned run/bundle sizes and retention decisions |
| `just cache gc --dry-run --keep 5` | Preview pruning; active/pending/running bundles protected |
| `--offline` | No Git/network fetching; Cargo and uv offline |
| `--profile NAME` / `--debug` | Explicit Cargo profile / development profile |
| `--jobs N` | Cargo job limit and pytest worker count |
| `--json` / `--explain` / `--timings` | Machine output / decisions / command durations |

**Source boundaries**

| Path                 | Value                                                                          |
| -------------------- | ------------------------------------------------------------------------------ |
| `crates/vtabs-core`  | Domain state, spaces, routing, native action policy, settings schema           |
| `crates/vtabs-ui`    | Ratatui composition, TachyonFX, input, menus and forms                         |
| `crates/vtabs-app`   | Per-window coordination, dirty scheduling, asynchronous ports                  |
| `crates/vtabs-store` | Storage protocol and SQLite helper                                             |
| `native/adapter`     | Private WezTerm API integration                                                |
| `native/patches`     | Generic native layout, surfaces, input and navigation hooks                    |
| `plugin`             | Optional Lua configuration and generated contracts                             |
| `tools/src` | Rust CLI, process runner, source/build state, packaging and updates |
| `tools/tests` | Rust-specific unit contracts |
| `tests/tools` | pytest/tui-test tooling behavior through the compiled CLI |
| `scripts/native.py` | Temporary forwarding shim for previously installed Python updaters |
| `tests`              | uv-managed pytest, production process boundaries and isolated native scenarios |

`vtabs-store` has no default features. The GUI links protocol types only; the `sqlite` feature builds the helper.

```sh
cargo test -p vtabs-store --features sqlite
cargo build --release --locked -p vtabs-store --features sqlite
cargo run --quiet --locked -p vtabs-core --bin gen-schema -- json
```

**Build state and installation**

| Name                   | Value                                                                                                      |
| ---------------------- | ---------------------------------------------------------------------------------------------------------- |
| `WEZ_VTABS_CACHE`      | `$XDG_CACHE_HOME/wez-vtabs-native`, `~/.cache/wez-vtabs-native`, or `%LOCALAPPDATA%/wez-vtabs-native`      |
| `WEZ_VTABS_INSTALL`    | `$XDG_DATA_HOME/wez-vtabs-native`, `~/.local/share/wez-vtabs-native`, or `%LOCALAPPDATA%/wez-vtabs-native` |
| `cache/upstream`       | Tool-owned upstream clone and Cargo target cache                                                           |
| `cache/worktree`       | Owned patched checkout; adapter changes synchronize in place                                                     |
| `cache/project`        | Installed updater's separate native-branch checkout; an ownership marker is required before replacement    |
| `cache/build.json`     | Separate source/compile/validation identities, toolchain/configuration and Cargo artifact paths                                                               |
| `install/versions`     | Immutable bundles; running processes keep their files                                                      |
| `install/active.json`  | Selected installed bundle                                                                                  |
| `install/pending.json` | Completed update selected by the next managed launch                                                       |
| `install/update.json`  | Last update attempt and result                                                                             |
| `install/update.log`   | Background build output                                                                                    |
| `cache/runs/ID/run.json` | Invocation, resolved revisions/locks, configuration, command logs and timings |
| `cache/runs/ID/source` | Project source snapshot for reproduction |
| `install/previous.json` | Previous active version for rollback |
| `install/wez-vtabs-launcher` | Stable dispatcher; versioned Rust tools own launch/update behavior |
| macOS launch entry     | `install/WezTerm Native.app`                                                                               |
| Linux launch entry     | `install/wez-vtabs` and `install/wez-vtabs.desktop`                                                        |
| Windows launch entry   | `install/wez-vtabs.cmd`                                                                                    |

Use the managed launch entry for updates between launches. Versioned application paths identify a particular build. Installed bundles contain their project source; native rebuilds require the toolchain. Rust changes require rebuilding; Lua configuration reloads normally.

Launch checks run asynchronously, at most daily. Installed updates fetch the recorded project branch (native by default) into a separate cache, then build against latest WezTerm main. A completed update becomes a separate version. Apply/build failures are recorded; no revision fallback or automatic patch rewriting occurs. Source-checkout commands build the current project files. The small three-OS workflow checks project changes and daily upstream changes.

Install platform dependencies using the selected upstream checkout's `get-deps` instructions. The workflow uses upstream `get-deps` on macOS/Linux and the Windows MSVC toolchain. See [WezTerm source builds](https://wezterm.org/install/source.html).

**Failure reproduction**

```sh
# Download and extract the CI tooling-reproduction artifact.
just repro /path/to/run/run.json
just repro /path/to/run/run.json --execute
just repro /path/to/run/run.json --execute --project-root /path/to/checkout
```

| Name | Value |
| --- | --- |
| Inspection | Prints invocation, commands, selected source and configuration |
| Execution | Separate `cache/reproductions/ID` source/cache/install; pinned recorded upstream |
| Build inputs | Source snapshot, project revision, resolved Cargo locks, compiler/configuration identity |
| Compatibility | Replay rejects incompatible compiler/target/profile/configuration inputs |
| Logs | Per-command stdout/stderr paths, exit status, elapsed milliseconds |
| CI | Failed/cancelled jobs upload reports and snapshots on all three platforms |
| Scope | Prepare/deps/build/check/test/package/generate/patch operations; install/launch inspection only |

**Prebuilt releases**

`just package` writes an adjacent `*.manifest.json`. Publish it beside its archive, then use `just update --manifest URL`. CI uploads both as artifacts; no release is published by local commands.

| Manifest field | Value |
| --- | --- |
| `schema_version` | `1` |
| `id`, `target`, `source_digest`, `upstream` | Exact bundle identity and source/upstream hashes |
| `project_source` | Recorded remote, branch and exact project revision |
| `archive` | Sibling archive filename or HTTPS URL |
| `sha256`, `size` | Archive integrity; extracted contents verified again |

**Test suite**

```sh
uv sync --locked
uv run --locked pytest -n 2
uv run --locked ruff check scripts tests
uv run --locked ruff format --check scripts tests
uv run --locked pytest -n 2 --run-luals
uv run --locked pytest -n 2 tests/integration --run-native --run-container \
  --native-bin-dir=/path/to/native/bin
```

| Check                   | Value                                                                                                                        |
| ----------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| Default suite           | Tooling, production Rust schema/storage processes, headless Lua plugin boundary and CLI PTYs                                 |
| Python tools            | pytest, pytest-asyncio, pytest-xdist, Ruff, tui-test; exact versions in `uv.lock`                                            |
| Worker count            | `-n 2`; each test receives separate state and temporary files                                                                |
| Tools binary | `--tools-bin=PATH`; built once under `target/pytest`, coordinated across workers |
| Rust binaries           | Cached project-only build coordinated across workers; `--rust-bin-dir=PATH` uses supplied `gen-schema` and `wez-vtabs-store` |
| Lua                     | `lua` or `luajit`; executes `plugin/init.lua` through its public configuration boundary                                      |
| LuaCATS                 | `--run-luals`; requires `lua-language-server`; valid and invalid public option examples                                      |
| Native binaries         | `--run-native --native-bin-dir=PATH`; prebuilt GUI, CLI, mux server and storage helper; focused CLI suites use recorded Cargo artifacts when no directory is supplied            |
| Native display          | Linux Xvfb and Openbox owned by the fixture; inherited desktop/session endpoints removed                                     |
| Mouse and screenshots   | xdotool and ImageMagick; only the owned headless display                                                                     |
| SSH mux                 | `--run-container`; loopback-only container, temporary keys, owned mux, Podman or Docker                                      |
| PTY                     | tui-test drives real CLI processes; the native group also exercises an isolated installed `wez-vtabs` launcher               |
| Startup/render/shutdown | Local and Unix mux native sessions with visible content, sidebar rendering and clean shutdown; TLS tab lifecycle             |
| Desktop                 | No suite or native scenario opens a GUI on the user's desktop                                                                |

**Extended native scenarios**

```sh
uv run --locked python -m tests.native.scenarios \
  --gui /path/to/wezterm-gui --helper /path/to/wez-vtabs-store \
  --workspace --chrome --edge-cases --hooks --effects --capture

uv run --locked python -m tests.native.scenarios \
  --gui /path/to/wezterm-gui --domain unix

uv run --locked python -m tests.native.ui_scenarios \
  --gui /path/to/wezterm-gui --helper /path/to/wez-vtabs-store \
  --output /tmp/vtabs-ui

uv run --locked python tests/native/tls_fixture.py --wezterm /path/to/wezterm -- \
  --gui /path/to/wezterm-gui --helper /path/to/wez-vtabs-store --workspace --effects
```

| Scenario              | Value                                                                                        |
| --------------------- | -------------------------------------------------------------------------------------------- |
| Geometry              | Resize/reversal, stable reservation, background tab dimensions and split identities          |
| Navigation            | Native indexed/negative activation, folders, spaces, drag/reorder and sidebar modes          |
| Settings              | Dedicated page, filtering, centered editors and preserved terminal geometry                  |
| Clipboard             | Real OS copy/paste in tab search, settings search and color editor; passive pointer movement |
| TLS                   | Temporary CA and certificates, mutual authentication and hostname verification               |
| Artifacts             | JSON report, sampled geometry, GUI/mux logs and owned-window screenshots                     |
| `--capture`           | Capture only the fixture's private X11 window                                                |
| `--baseline-gui PATH` | Compare split sizing with stock at the same viewport                                         |
| `--trace-mux`         | Additional transport/layout traces; changes timing                                           |
| Performance boundary  | CPU/render samples exclude GPU completion and physical display latency                       |

**Storage diagnostics**

| Env               | Value                                                                                              |
| ----------------- | -------------------------------------------------------------------------------------------------- |
| `WEZ_VTABS_STORE` | Explicit helper path; defaults to the GUI executable's sibling                                     |
| `WEZ_VTABS_DB`    | Explicit SQLite path; defaults to the platform local-data directory under `wez-vtabs/state.sqlite` |

Requests and responses are versioned and bounded. Field revisions reject stale writes atomically; tombstones retain revisions. Session-scoped live state requires a verified incarnation. Private live-tab state is excluded; explicit catalog/settings edits remain shared and durable. Database work never runs in the resize or native activation path.
