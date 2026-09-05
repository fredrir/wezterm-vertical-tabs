# Development

| Name | Value |
| --- | --- |
| Toolchain | Stable Rust, Git, Python 3.10+, platform C/C++ build tools |
| Upstream | Latest `wezterm/wezterm` `main`; resolved on every build |
| GUI | WezTerm renderer with the native patch series and project Rust application |
| UI | Retained Ratatui text, native rounded geometry and finite TachyonFX effects |
| Persistence | `wez-vtabs-store`; bundled SQLite, asynchronous bounded JSON requests |
| Lua | Optional configuration, generated schema/types, semantic hooks |

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

On Windows, recipes use `py -3`. The direct entry point is `python3 scripts/native.py` (`py -3 scripts/native.py` on Windows).

| Command | Value |
| --- | --- |
| `just build` | Fetch main, check/apply ordered patches, stage adapter, build, run integrated native adapter/host tests |
| `just dev` | Build and launch a separate GUI process |
| `just check` | Rust format/tests/Clippy, generated Lua contracts, native tooling tests |
| `just generate` | Generate `plugin/schema.lua`, `plugin/types.lua` and `docs/options.md` from Rust |
| `just package` | Native platform bundle and `.zip`/`.tar.gz` in `dist/` |
| `just install` | Install an immutable bundle and managed launch entry |
| `just install --bundle PATH` | Install an already extracted native bundle |
| `just launch` | Select a completed pending update and launch the native GUI |
| `just update` | Build latest main and select the completed bundle for subsequent launches |
| `just doctor` | Tool versions, build metadata, update status and paths |
| `--debug` | Use Cargo's debug profile for build/dev/package |

**Source boundaries**

| Path | Value |
| --- | --- |
| `crates/vtabs-core` | Domain state, spaces, routing, native action policy, settings schema |
| `crates/vtabs-ui` | Ratatui composition, TachyonFX, input, menus and forms |
| `crates/vtabs-app` | Per-window coordination, dirty scheduling, asynchronous ports |
| `crates/vtabs-store` | Storage protocol and SQLite helper |
| `native/adapter` | Private WezTerm API integration |
| `native/patches` | Generic native layout, surfaces, input and navigation hooks |
| `plugin` | Optional Lua configuration and generated contracts |
| `scripts/native.py` | Cross-platform build, package, install and update entry point |
| `tests/native` | Tooling contracts and opt-in native GUI scenarios |

`vtabs-store` has no default features. The GUI links protocol types only; the `sqlite` feature builds the helper.

```sh
cargo test -p vtabs-store --features sqlite
cargo build --release --locked -p vtabs-store --features sqlite
cargo run --quiet --locked -p vtabs-core --bin gen-schema -- json
```

**Build state and installation**

| Name | Value |
| --- | --- |
| `WEZ_VTABS_CACHE` | `$XDG_CACHE_HOME/wez-vtabs-native`, `~/.cache/wez-vtabs-native`, or `%LOCALAPPDATA%/wez-vtabs-native` |
| `WEZ_VTABS_INSTALL` | `$XDG_DATA_HOME/wez-vtabs-native`, `~/.local/share/wez-vtabs-native`, or `%LOCALAPPDATA%/wez-vtabs-native` |
| `cache/upstream` | Tool-owned upstream clone and Cargo target cache |
| `cache/worktree` | Disposable patched checkout; replaced by prepare/build |
| `cache/project` | Installed updater's separate native-branch checkout; an ownership marker is required before replacement |
| `cache/build.json` | Diagnostic source/upstream hashes and target |
| `install/versions` | Immutable bundles; running processes keep their files |
| `install/active.json` | Selected installed bundle |
| `install/pending.json` | Completed update selected by the next managed launch |
| `install/update.json` | Last update attempt and result |
| `install/update.log` | Background build output |
| `install/runtime.json` | Python executable selected by install; used by native startup checks |
| macOS launch entry | `install/WezTerm Native.app` |
| Linux launch entry | `install/wez-vtabs` and `install/wez-vtabs.desktop` |
| Windows launch entry | `install/wez-vtabs.cmd` |

Use the managed launch entry for updates between launches. Versioned application paths identify a particular build. Installed bundles contain their project source; native rebuilds require the toolchain. Rust changes require rebuilding; Lua configuration reloads normally.

Launch checks run asynchronously, at most daily. Installed updates fetch the recorded project branch (native by default) into a separate cache, then build against latest WezTerm main. A completed update becomes a separate version. Apply/build failures are recorded; no revision fallback or automatic patch rewriting occurs. Source-checkout commands build the current project files. The small three-OS workflow checks project changes and daily upstream changes.

Install platform dependencies using the selected upstream checkout's `get-deps` instructions. The workflow uses upstream `get-deps` on macOS/Linux and the Windows MSVC toolchain. See [WezTerm source builds](https://wezterm.org/install/source.html).

**GUI verification**

```sh
python3 tests/native/scenarios.py \
  --gui /path/to/wezterm-gui \
  --helper /path/to/wez-vtabs-store \
  --baseline-gui /path/to/same-main-stock/wezterm-gui \
  --workspace --chrome --edge-cases

python3 tests/native/scenarios.py \
  --gui /path/to/wezterm-gui \
  --helper /path/to/wez-vtabs-store \
  --domain unix

python3 tests/native/scenarios.py \
  --gui /path/to/wezterm-gui \
  --helper /path/to/wez-vtabs-store \
  --domain ssh --ssh-config /path/to/disposable-ssh-domain.json

python3 tests/native/ssh_fixture.py --wezterm /path/to/wezterm -- \
  --gui /path/to/wezterm-gui --helper /path/to/wez-vtabs-store \
  --workspace --hooks --effects --resize-rounds 3

python3 tests/native/moves.py --gui /path/to/wezterm-gui \
  --domain unix --external --output /tmp/vtabs-moves
```

| Check | Value |
| --- | --- |
| Isolation | Unique workspace/window class, copied executable, separate XDG paths and SQLite DB |
| Geometry | Resize/reversal samples, fixed sidebar reservation, background-tab dimensions |
| Splits | Pane identities, topology, exact sizing comparison with stock at equivalent viewport |
| Native actions | Indexed and negative activation using native WezTerm actions |
| Features | Empty spaces, new tabs, idle surface reuse, durable settings, private-state exclusion and native private-tab environment |
| SSH fixture | JSON `SshDomain` configuration for a disposable test mux; ordinary host authentication applies |
| Localhost SSH helper | POSIX host with `sshd`/OpenSSH; temporary unprivileged server, keys and mux; no user SSH configuration changes |
| Move regression | `moves.py`: local split or Unix single-pane move; `--external` uses a second Unix mux client. Checks ownership, remote resizing, Reopen and process survival; helper/server/CLI default to GUI siblings |
| Artifacts | `report.json`, `samples.jsonl`, GUI/mux logs and fixture configuration |
| `--geometry-only` | Restrict the run to geometry and native navigation |
| `--workspace` | Switch native workspaces out/back and verify selected space, membership and pins |
| `--chrome` | Integrated title controls and nonzero terminal padding |
| `--edge-cases` | Font size, fullscreen, pane zoom, right sidebar, rail reservations and tiny windows |
| `--trace-mux` | Client/server and GUI resize traces; tracing changes timing |
| `--hooks --effects` | All semantic hooks, process routing metadata and default finite transitions; verify idle cache reuse |
| `--resize-rounds 5` | Five dense resize/reversal rounds in the same GUI window |
| `--capture` | Capture only the fixture's macOS window; requires OS screen-capture access |
| Measurement boundary | GUI state sampling; GPU presentation, physical key latency, OS drag, DPI/IME and visual quality need native profiling/manual checks |

**Storage diagnostics**

| Env | Value |
| --- | --- |
| `WEZ_VTABS_STORE` | Explicit helper path; defaults to the GUI executable's sibling |
| `WEZ_VTABS_DB` | Explicit SQLite path; defaults to the platform local-data directory under `wez-vtabs/state.sqlite` |

Requests and responses are versioned and bounded. Field revisions reject stale writes atomically; tombstones retain revisions. Session-scoped live state requires a verified incarnation. Private live-tab state is excluded; explicit catalog/settings edits remain shared and durable. Database work never runs in the resize or native activation path.

**TLS and physical input fixtures**

```sh
python3 tests/native/tls_fixture.py --wezterm /path/to/wezterm -- \
  --gui /path/to/wezterm-gui --helper /path/to/wez-vtabs-store \
  --workspace --effects

python3 tests/native/ui_scenarios.py --gui /path/to/wezterm-gui \
  --helper /path/to/wez-vtabs-store --output /tmp/vtabs-ui
```

| Name | Value |
| --- | --- |
| TLS fixture | Temporary CA, server and user certificates; mutual authentication and hostname verification |
| Linux UI fixture | Isolated Xvfb display, Openbox, xdotool and ImageMagick; real keyboard, click, drag and screenshot checks |
| UI artifacts | Sidebar, settings, search and tooltip screenshots; JSON report and isolated logs |
