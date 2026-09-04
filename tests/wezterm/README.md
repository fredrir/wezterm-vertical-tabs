# Real WezTerm mux-domain tests

> Opt-in: these tests launch a GUI. Bare `pytest`, `just test`, `just check`, and the current CI
> workflow exclude `tests/wezterm`.

This suite treats WezTerm, `wezterm-mux-server`, the plugin, and `wez-vtabs` as one user-visible
system. Every test owns a foreground mux server on a private Unix socket and a separately owned GUI
client. It drives both endpoints through `wezterm cli` and asserts pane/tab behavior rather than Lua
state, log messages, or implementation call counts.

## Run it

Install WezTerm and [`uv`](https://docs.astral.sh/uv/), then run from a graphical macOS session or a
Linux session with `DISPLAY`/`WAYLAND_DISPLAY`:

```sh
just test-wezterm-e2e                 # full Unix-domain suite, two workers
just test-wezterm-e2e-smoke           # representative opt-in subset
VTABS_E2E_WORKERS=1 just test-wezterm-e2e -k reconnect
```

The commands build the debug backend once, install the exact lockfile, and fail when WezTerm, its
matching `wezterm-mux-server`, the backend, or a graphical session is unavailable. A direct run is:

```sh
WEZ_VTABS_BIN="$PWD/backend/target/debug/wez-vtabs" \
  uv run --frozen pytest -q -n 2 -m "not ssh_mux_e2e" tests/wezterm
```

The optional SSH-domain contract is Linux-only because the source-built host backend is mounted
into an Ubuntu container. It needs Docker and a host WezTerm version matching the image:

```sh
just test-wezterm-ssh-e2e
```

To test another exact host/container build, provide both
`VTABS_SSH_WEZTERM_DEB_URL` and `VTABS_SSH_WEZTERM_DEB_SHA256`.

## Contracts covered

- the GUI really attaches to a standalone mux and gets one rendering sidebar per tab;
- disconnect/reconnect preserves server panes and adopts the surviving sidebar;
- a new mux tab converges independently;
- hidden sidebars restore without replacing user content;
- window resize and manual divider width preserve the user's sidebar geometry;
- a split accidentally started from the sidebar is rescued into usable content;
- a held tab-switch key leaves every tab's content and single sidebar intact and the GUI attached;
- the settings page closes as a whole tab, never leaving a tab holding only its sidebar;
- local and standalone-mux tabs remain on their own endpoints;
- the Docker lane executes a tab and its sidebar through a real localhost SSH domain.

The marker split is intentional: `smoke` is the short representative subset, `wezterm_e2e` names
every real-GUI test, and `ssh_mux_e2e` isolates the more expensive container scenario.

## Isolation and determinism

The function-scoped harness creates a short `/tmp/vt-*` root for every test with private HOME/XDG
directories, GUI socket directory, Unix mux socket, class name, logs, and process groups. The mux
server never auto-starts, and teardown signals only PIDs created by that fixture. Tests share no
mutable panes, workspaces, sockets, or GUI processes and are safe to run in either order or with two
xdist workers.

Two workers are the measured default: on the 2026-09-02 macOS reference run, the eight Unix-domain
tests took 23.4 seconds serially and 12.8 seconds with `-n 2`. More workers are deliberately not the
default because GUI/display contention grows faster than this small suite.

All convergence uses monotonic semantic waits. A condition must remain true for `stable_for` before
the test proceeds; test bodies contain no fixed sleeps. IDs and timings are never assertions. The
only random values name private resources.

## Failure artifacts

A failed test prints and retains a directory under `.pytest-artifacts/wezterm/` containing:

- every GUI process log, the mux-server log, and backend log;
- final GUI/mux topology and mux-client diagnostics;
- an ordered history of every CLI command, result, duration, and timeout;
- SSH container/backend logs for the Docker lane.

Successful tests remove their private runtime roots.

Keep new scenarios outcome-driven: one question per test, public CLI/terminal observations, explicit
failure cases, and the smallest stable wait. Do not assert private Lua state, debug log text, exact
timings, proxy IDs across endpoints, screenshot goldens, or test order. A visual or internal refactor
should leave these tests unchanged when behavior is unchanged.

## CI policy

| Lane | Policy |
| ---- | ------ |
| default CI | headless plugin and TUI suites only |
| real GUI | local opt-in via `just test-wezterm-e2e` |
| GUI smoke | local opt-in via `just test-wezterm-e2e-smoke` |
| SSH mux | local opt-in via `just test-wezterm-ssh-e2e` |
