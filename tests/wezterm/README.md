# Real WezTerm smoke test

This directory contains one deliberately small integration test. It starts a real GUI with a
private home, XDG directories, runtime socket directory, class, and workspace. It never discovers
or stops WezTerm processes by name.

The test is opt-in because a GUI launch is much slower than the PTY suite and needs a graphical
session:

```sh
VTABS_WEZTERM_E2E=1 uv run --frozen pytest -q tests/wezterm
```

The source-built backend defaults to `backend/target/debug/wez-vtabs`. Override that explicit path
with `WEZ_VTABS_BIN=/absolute/path/to/wez-vtabs`; the test never resolves the backend from `PATH` and
does not run Cargo.

The assertion intentionally ignores text labels, colors, spacing, and dimensions. It only checks
the user-visible contract that each tab has content beside one non-blank sidebar.
