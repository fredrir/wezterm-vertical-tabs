# Development

## Workspace layout


| Crate             | Responsibility                                                                                                           |
| ----------------- | ------------------------------------------------------------------------------------------------------------------------ |
| `vtabs-protocol`  | wire DTOs (`Command`, `Event`, typed `Intent`), `VERSION`, bounds, and the generated Lua protocol mirror                 |
| `vtabs-engine`    | pure state, theme/spaces policy, interaction, strip geometry, layout/rendering, and the canonical settings/config schema |
| `vtabs-runtime`   | stdin parser, atomic generation state machine, terminal guard, event emission, signals and paint — the sole stdout writer |
| `vtabs-zen`       | rounded-frame rendering and PNG encoding for the `wez-vtabs frame` subcommand                                           |
| `wez-vtabs` (bin) | `frame`/`settings normalize` subcommand dispatch and the runtime entrypoint                                              |

The former `vtabs-core`, `vtabs-input`, `vtabs-theme`, and `vtabs-view` crates are now modules of
`vtabs-engine`. That keeps wire compatibility separate without forcing internal domain and scene
types through crate boundaries. The engine produces typed intents; the runtime retains one
centralized legacy `do` downgrade for older clients.

```sh
cd backend
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
cargo build --release      # target/release/wez-vtabs
```

## Architecture and ownership

Lua gathers one immutable mux snapshot per update and sends raw host facts. A capable client wraps
the changed `config`, `theme`, `spaces`, `model`/`settings`, and optional `menu` sections in one
generation's `begin`/`commit`. The runtime validates that staging state and publishes it once, so a
batch cannot paint or hit-test a mixture of revisions. Legacy clients may still send the original
sections immediately; `spaces` uses the atomic `spaces_policy` path only.

Rust owns the policies that must agree with painting: theme resolution and contrast, spaces
validation/routing/visibility/theme selection, focused key resolution, pointer hit-testing, and
derived strip geometry. Lua owns the WezTerm API boundary, mux mutations, executing user hooks,
and host projection. In particular:

- `ThemeMsg` carries a raw effective palette, typed overrides, window-private state, and a
  hook-needed flag. Rust resolves the base. When a capable client requests a theme hook, commit
  pauses for one generation-matched `theme_hook_result`, then publishes and paints exactly once.
  A lost request is replayed once after 500 ms and then falls back to the same deterministic base.
  Every committed effective answer is exposed as `theme_resolved` for Lua and Zen host projection.
- `SpacesMsg` carries raw definitions plus a complete window census and current assignment facts.
  `vtabs-engine::spaces` validates and matches rules/globs, expands templates, owns sticky/manual
  routing and dynamic admission, derives summaries/visibility, and selects the active theme layer.
  A needed Lua route hook pauses the same generation as one batched request; its exact answer is
  resolved before any theme-hook request, and the runtime still publishes and paints only once.
  Lua retains only mux/window state application and bounded hook/result caches. Without the
  `spaces_policy` capability it warns once and exposes all tabs unpartitioned; there is no Lua
  policy fallback.
- The engine emits flattened, variant-specific `intent` events. Focus-mode `r`, `J`/`K`, and
  `[`/`]` resolve to rename, move-tab, and switch-space intents in Rust; only unfocused host
  forwarding uses a raw `key` event.
- Lua sends raw pane metrics and window-chrome facts. `vtabs-engine::geom` computes rows, cell
  dimensions, toggle positions, and the rail reserve; Lua only applies the returned reserve to the
  host window.
- Lua sends raw title, override, pane-title, process, cwd, user, host, and domain facts plus menu
  subject ids. `vtabs-engine::enrich` alone derives card titles/metadata and every menu header,
  keeping shell/remote/title precedence in one policy.

With `VTABS_LOG` set, each successful terminal write logs cumulative `commits`, `paints`, and
`bytes` beside the current size/revision. A hook-delayed generation increments `commits` only when
published; an identical repaint that writes no bytes increments neither paint counter.

## Dependency spikes

The optional PNG and command-line parser dependencies were measured on 2026-09-02 on macOS
arm64 with Rust 1.97.1. Each candidate was built in a separate `mktemp -d` copy of the same source
snapshot, excluding `.git` and `backend/target`. Release builds use the workspace's size profile.

| Candidate | Release binary | Representative output | 30-run frame time | Decision |
| --- | ---: | ---: | ---: | --- |
| Custom PNG encoder | 771,696 B | 138,768 B | 127.1 ms ± 1.4 ms | Baseline |
| `png` 0.18.1, `Fast` + `Sub` | 838,048 B | 150,608 B | 67.1 ms ± 3.9 ms | Adopted |
| `lexopt` 0.3.2 | 788,304 B | unchanged | not render-path relevant | Rejected |

The `png` candidate adds 66,352 bytes and remains below the 1.25 MiB (1,310,720-byte) release
cap. The output was identified by `file` as non-interlaced 2880×1800 RGBA8, decoded by `sips`
at the same dimensions, and is covered by an in-process RGBA8 round-trip test. Its output is 8.5%
larger than the custom encoder's but its measured render is about 47% faster.

`lexopt` adds a modest 16,608 bytes, but it did not clear the other adoption gate: preserving the
four-value `--card`, finite-number checks, colour diagnostics, and permissive role scan changed 50
lines in and 37 lines out (net +13). The manual parsers remain smaller and clearer for these two
options, so the dependency was not adopted.

The measurements can be repeated from an isolated source copy with:

```sh
cd backend
cargo build --release --locked -p wez-vtabs
wc -c target/release/wez-vtabs
target/release/wez-vtabs frame \
  --w 2880 --h 1800 --card 420 24 2420 1752 \
  --radius 20 --border-width 1 \
  --fill '#1e1e2e' --card-fill '#11111b' --border '#45475a' \
  --out /tmp/vtabs-spike-frame.png
wc -c /tmp/vtabs-spike-frame.png
file /tmp/vtabs-spike-frame.png
sips -g pixelWidth -g pixelHeight /tmp/vtabs-spike-frame.png
hyperfine --warmup 5 --runs 30 \
  "target/release/wez-vtabs frame --w 2880 --h 1800 \
  --card 420 24 2420 1752 --radius 20 --border-width 1 \
  --fill '#1e1e2e' --card-fill '#11111b' --border '#45475a' \
  --out /tmp/vtabs-spike-frame.png"
```

## Plugin

```sh
cd plugin
lua tests/run.lua
luacheck init.lua vtabs tests
stylua --check init.lua vtabs tests
```

## Black-box TUI tests

The Python suite drives the real `wez-vtabs` binary through an isolated pseudo-terminal. Install
[`uv`](https://docs.astral.sh/uv/) and run:

```sh
just test-tui
just test-tui -k resize          # focused local run
```

`just test-tui` builds the debug `wez-vtabs` executable once, installs exactly the dependencies in
`uv.lock`, and gives that executable to pytest through `WEZ_VTABS_BIN`. Each test still launches a
new process and PTY session, so mutable terminal state cannot leak between tests. A direct pytest
run may omit `WEZ_VTABS_BIN`; the session fixture then performs one fallback Cargo build for the
whole run.

These tests protect behavior at the terminal boundary: public OSC user-variable events, caller-
supplied content appearing or disappearing, meaningful interaction results, resize/reflow, input
recovery, bounds handling, and clean shutdown. They deliberately do not pin borders, margins,
spacing, coordinates, built-in button labels, or whole-screen snapshots. A visual refactor should
only require a test change when it changes an actual user-facing contract.

Keep scenarios short and outcome-driven. Use fixed terminal sizes and explicit waits, never sleeps,
network access, wall-clock assertions, or shared sessions. The full suite is part of `just test`;
CI runs it on Linux in the existing Rust job and runs only the representative `smoke` subset on
macOS to keep CPU and runner use modest.

For the deliberately small, real-GUI smoke test, install WezTerm and run this from a macOS desktop
session or a Linux graphical session (`DISPLAY` or `WAYLAND_DISPLAY` must be available):

```sh
just test-wezterm-e2e
```

That explicit command sets the suite's opt-in gate, builds the backend once, and starts an isolated
WezTerm instance using `plugin/tests/wezterm-e2e.lua`. It is not a normal PR gate: hosted GUI setup
would add a comparatively slow package download and display-server dependency to a fast PTY suite.
See `tests/wezterm/README.md` for the isolation and assertion policy.

There is intentionally no historical WezTerm-version matrix. The maintained test surface is the
current Rust/Lua unit suite, focused PTY contracts, and the opt-in real-GUI smoke test above.

## Generated Lua mirrors

```sh
just generate
cargo run -q -p vtabs-protocol --bin gen-lua -- --check
cargo run -q -p vtabs-engine --bin gen-config -- --check
```

`plugin/vtabs/gen/protocol.lua` mirrors bounded wire constants plus the theme-field and typed-
intent inventories used at the Lua boundary. `plugin/vtabs/gen/schema.lua` mirrors the typed
descriptors in `vtabs-engine`; `plugin/vtabs/schema.lua` contains only generic dotted-key and
default-building helpers. Generated documentation consumes that same Lua mirror.

Persistence has one intentional host boundary. During `apply_to_config`, Lua calls an already
installed local `wez-vtabs settings normalize` directly when one is resolvable. A bounded request
file lives in a fresh `0700` directory, is restricted to `0600` before config bytes are written,
and is removed with the directory after the synchronous call. No bootstrap, build, download,
remote command, or WezTerm CLI participates. Rust parses/filters persistence v1, migrates aliases,
and resolves `defaults < stored < opts`; Lua restores functions, userdata, and cyclic config-as-code
values by reference only for Function/Any/open/callback-bearing owners. Typed non-JSON values are
reported to Rust and reset to their descriptor defaults. The response must match the normalizer
protocol, plugin version, and generated schema identity before Lua adopts it. A missing/older
backend falls back to the generated-schema Lua path, which is why a fresh installation remains
network-free at config evaluation time. `settings.persist=false` disables writes, not loading an
existing file.

Once the settings page is live, Rust exclusively canonicalizes and validates values, computes
changes, mutates the document, and serializes the complete deterministic JSON body; Lua performs
the final atomic, private filesystem write. On Unix that writer uses a fresh `0700` guard directory
beside the destination, a `0600` file restricted before content is written, and a same-filesystem
rename; any permission, write, flush, close, or rename failure leaves the destination untouched.
The generated 512 KiB bound is enforced before Rust publishes a commit and again before Lua
writes, so every accepted persisted body remains readable on the next boot.

Lua's boot fallback can only check a configured settings path before `io.open`; Lua exposes no
portable no-follow open. Therefore a custom settings file must live in a directory trusted by the
user. The private normalizer handoff itself is opened and validated by Rust with no-follow flags,
and live writes replace from a fresh private sibling directory rather than following the target.

## Deliberately held decisions

- The long-term backend boundary is undecided: neither a mux adapter nor a broader `wezterm cli`
  backend has been selected. The existing narrow kill/rescue CLI bridge is not that decision.
- Direct dependencies on `wezterm-gui`/`wezterm-mux-server` and their version-pinning policy remain
  on hold until the backend boundary is chosen.
- Broad CI/CD expansion and a historical WezTerm compatibility matrix remain on hold. Keep checks
  small and contract-focused rather than multiplying platform/version combinations.

## `just check`

```sh
just check
```

## Lints

```sh
just lint
```

## Options

```sh
just docs                       # regenerate the options table
lua scripts/gen-docs.lua --check  # fail when it is stale (run by `just lint`)
```

| descriptor field       | meaning                                                                                        |
| ---------------------- | ---------------------------------------------------------------------------------------------- |
| `key`                  | dotted path, e.g. `padding.top`, `backend.repo`                                                |
| `type`                 | `number` `string` `boolean` `enum` `table` `function` `any`                                    |
| `default`              | omitted when the option has none                                                               |
| `enum` `min` `max`     | validation; `alias` maps values users already write onto the canonical one                     |
| `container` `open`     | `open` containers (`theme`, `icon_map`, `keys`, `private.env`) do not enumerate their children |
| `docs`                 | `false` hides the row from the generated table                                                 |
| `shown`                | overrides the rendered default cell, backticks included                                        |
| `label` `group` `help` | settings UI and docs text                                                                      |
| `host_key`             | WezTerm configuration key affected by this option; generated for Lua host projection           |
| `policy_paths`         | exact open-table child paths that share the descriptor's host/apply policy                      |
| `apply_mode`           | live settings policy: `instant`, `override`, or `reload`                                        |

## Dev loop

Needs [`just`](https://github.com/casey/just) and [`watchexec`](https://github.com/watchexec/watchexec).

```sh
just    
just dev          
just dev --live
just doctor    
just restart   
```


## Handlers are coroutines



| Fact                                                                                                           | Probe line                                                                  |
| -------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------- |
| every `wezterm.on` emission, `call_after` and `action_callback` is its own Lua thread, `main=false`, yieldable | distinct `thread:` per `status`, `A#n`, `B#n`, `callback`                   |
| `wezterm.sleep_ms` yields to other handlers                                                                    | `B#3`..`B#7` land between `sleep start` and `sleep end`                     |
| a thread keeps its identity across its own awaits                                                              | `split before`/`split after`, `sleep start`/`sleep end` share one `thread:` |
| finished threads are recycled                                                                                  | a `thread:` value reappears on a later, unrelated emission                  |

```sh
HOME=$(mktemp -d) WEZTERM_LOG=info wezterm --config-file scripts/probe-coroutines.lua start --always-new-process 2>&1 | grep probe
```

## Crash bisect

| Trigger                         | Where                                                               |
| ------------------------------- | ------------------------------------------------------------------- |
| `top_level = true` split        | `sidebar_attach.lua` `attach`                                       |
| close by activation             | `sidebar_attach.lua` `close_pane_by_activation`, `close_orphan`     |
| `AdjustPaneSize`, `Multiple`    | `geometry.lua` `correct`                                            |
| `set_config_overrides`          | `view.lua` `apply_titlebar_band`, `frame.lua` `install`, `page.lua` |
| `move_to_new_window`            | `actions.lua` `tear_off`                                            |
| `set_inner_size`                | `sidebar_attach.lua` `fit_to_window`                                |
| `cli split-pane --move-pane-id` | `sidebar_rescue.lua` `rescue_splits`                                |

## Applying a build

```sh
just deploy                  
just deploy --from-prd
just deploy --from-release    # newest published GitHub release
```

`just tag` merges `dev` into `main`, creates the next patch release, then fast-forwards and pushes
`dev` to the same release commit. This keeps both branches' version metadata in sync.

## Local development config

```lua
package.path = "/path/to/wezterm-vertical-tabs/plugin/?.lua;" .. package.path
local vtabs = dofile "/path/to/wezterm-vertical-tabs/plugin/init.lua"
vtabs.apply_to_config(config, {
  backend = { path = "/path/to/wezterm-vertical-tabs/backend/target/release/wez-vtabs" },
  debug = true, 
})
```

## Bootstrap environment

| Variable        | Description                          |
| --------------- | ------------------------------------ |
| `VTABS_TARGET`  | Rust triple                          |
| `VTABS_REPO`    | Repository URL                       |
| `VTABS_VERSION` | Release tag without `v`              |
| `VTABS_SRC`     | Backend crate for the cargo fallback |
| `VTABS_BUILD`   | `0` disables it                      |
| `VTABS_BIN`     | Explicit binary                      |
| `VTABS_USERVAR` | User variable                        |
