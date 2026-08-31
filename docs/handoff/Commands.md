## Commands

| Command/Workflow | Description |
| --- | --- |
| `just check` | cargo test/fmt/clippy + lua tests + luacheck + stylua + gen-docs --check |
| `timeout 600 xvfb-run -a sh plugin/tests/e2e.sh` | e2e local mode |
| `timeout 900 xvfb-run -a sh plugin/tests/e2e.sh mux` | e2e mux mode; fails by hanging to timeout — re-run a red before believing it |
| `sh plugin/tests/stress.sh` | stress suite; `VTABS_E2E_MACOS=1`, `VTABS_STRESS_SOFT=1` |
| `sh scripts/baseline.sh --pin` | pin frames/shots/geometry/stress reference (`.claude/team/baseline`) |
| `sh scripts/baseline.sh --check --only frames` | fast neutrality gate (0.76s) |
| `sh scripts/flake.sh N [local\|mux] [e2e\|stress]` | N-run flake counter |
| `sh scripts/screenshot.sh` | 24+ table-driven UI states → `.claude/team/shots/` |
| `just deploy` + `frame = "zen"` | live zen-frame test in your own WezTerm |
