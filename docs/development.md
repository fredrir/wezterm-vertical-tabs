# Development

| Name        | Value                                                                                          |
| ----------- | ---------------------------------------------------------------------------------------------- |
| Toolchain   | Stable Rust, Git, Python 3.10+ for builds; uv and Python 3.12+ for tests; platform C/C++ tools |
| Upstream    | Latest `wezterm/wezterm` `main`; resolved on every build                                       |
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

Recipes use `uv run --locked python` on every platform. Direct builds also support `python3 scripts/native.py` (`py -3 scripts/native.py` on Windows).

| Command                      | Value                                                                                                   |
| ---------------------------- | ------------------------------------------------------------------------------------------------------- |
| `just build`                 | Fetch main, check/apply ordered patches, stage adapter, build, run integrated native adapter/host tests |
| `just dev`                   | Build and launch a separate GUI process                                                                 |
| `just check`                 | Rust format/tests/Clippy, generated Lua contracts, Ruff and pytest behavior tests                       |
| `just generate`              | Generate `plugin/schema.lua`, `plugin/types.lua` and `docs/options.md` from Rust                        |
| `just package`               | Native platform bundle and `.zip`/`.tar.gz` in `dist/`                                                  |
| `just install`               | Install an immutable bundle and managed launch entry                                                    |
| `just install --bundle PATH` | Install an already extracted native bundle                                                              |
| `just launch`                | Select a completed pending update and launch the native GUI                                             |
| `just update`                | Build latest main and select the completed bundle for subsequent launches                               |
| `just doctor`                | Tool versions, build metadata, update status and paths                                                  |
| `--debug`                    | Use Cargo's debug profile for build/dev/package                                                         |

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
| `scripts/native.py`  | Cross-platform build, package, install and update entry point                  |
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
| `cache/worktree`       | Disposable patched checkout; replaced by prepare/build                                                     |
| `cache/project`        | Installed updater's separate native-branch checkout; an ownership marker is required before replacement    |
| `cache/build.json`     | Diagnostic source/upstream hashes and target                                                               |
| `install/versions`     | Immutable bundles; running processes keep their files                                                      |
| `install/active.json`  | Selected installed bundle                                                                                  |
| `install/pending.json` | Completed update selected by the next managed launch                                                       |
| `install/update.json`  | Last update attempt and result                                                                             |
| `install/update.log`   | Background build output                                                                                    |
| `install/runtime.json` | Python executable selected by install; used by native startup checks                                       |
| macOS launch entry     | `install/WezTerm Native.app`                                                                               |
| Linux launch entry     | `install/wez-vtabs` and `install/wez-vtabs.desktop`                                                        |
| Windows launch entry   | `install/wez-vtabs.cmd`                                                                                    |

Use the managed launch entry for updates between launches. Versioned application paths identify a particular build. Installed bundles contain their project source; native rebuilds require the toolchain. Rust changes require rebuilding; Lua configuration reloads normally.

Launch checks run asynchronously, at most daily. Installed updates fetch the recorded project branch (native by default) into a separate cache, then build against latest WezTerm main. A completed update becomes a separate version. Apply/build failures are recorded; no revision fallback or automatic patch rewriting occurs. Source-checkout commands build the current project files. The small three-OS workflow checks project changes and daily upstream changes.

Install platform dependencies using the selected upstream checkout's `get-deps` instructions. The workflow uses upstream `get-deps` on macOS/Linux and the Windows MSVC toolchain. See [WezTerm source builds](https://wezterm.org/install/source.html).

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
| Rust binaries           | Cached project-only build coordinated across workers; `--rust-bin-dir=PATH` uses supplied `gen-schema` and `wez-vtabs-store` |
| Lua                     | `lua` or `luajit`; executes `plugin/init.lua` through its public configuration boundary                                      |
| LuaCATS                 | `--run-luals`; requires `lua-language-server`; valid and invalid public option examples                                      |
| Native binaries         | `--run-native --native-bin-dir=PATH`; prebuilt GUI, CLI, mux server and storage helper; no implicit WezTerm build            |
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
