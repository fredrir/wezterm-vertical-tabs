## Current State

`dev` clean @ `c1b1b75`, contains all merged work (301 Lua + 93 Rust tests, luacheck 0, both e2e modes green at last gate). Agent team dead: monthly spend limit, session limit resets 23:50 Europe/Oslo.

### What's Working

| Feature                                  | Status                      | Justification                                      |
| ---------------------------------------- | --------------------------- | -------------------------------------------------- |
| P0 bug batch                             | fixed                       | each pinned by a test; restart adoption 332ms      |
| Cards/ghost/strip/popover/tinted sidebar | live                        | design-reviewer verdicts on pixels                 |
| Settings page + precedence               | live                        | precedence proven on pixels                        |
| Zen frame prototype                      | live behind `frame = "zen"` | rounded card measured on pixels                    |
| Refactor steps 1–4, 9, 10                | merged                      | frames byte-identical to pinned baseline `58658e8` |
| Baseline/screenshot/stress/flake harness | working                     | `--check --only frames` = 0.76s                    |

### What's Not Working

| Title                                 | Description                                                                                                                                                                                                                              | Suspected cause                |
| ------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------ |
| Stranded uncommitted work             | Implementer-2's Zen-fix + refactor batch sits UNCOMMITTED in `.claude/worktrees/agent-aa383cd4d130cce76` (11 files) and `agent-a219db4f7382217fb` (10 files); agent died mid-write. Assess before touching those worktrees.              | spend limit killed the agent   |
| Zen one-surface mismatch              | Sidebar interior `#262634`, frame margin `#29293A`; harness soft-red `XFAIL: zen`. Fix was in the stranded batch.                                                                                                                        | two colour sources not unified |
| Zen inset/Z5/Z6/Z7                    | Card overhangs pane rect by 6 device px; `frame.lua:272` clobbers `window_padding` (breaks macOS titlebar band); PNG path needs pid+colour hash; `#theirs > 1` refusal missing. Security findings: `.claude/team/security-review-r2.md`. | fixes stranded (above)         |
| B7 adoption                           | Band-clamped target (18) can be adopted over a dragged 40; fix = record `last_target[wid]`, add `cols ~= last_target[wid]` to adoption.                                                                                                  | fix stranded (above)           |
| Stress XFAILs undiagnosed             | `split_net`, `close_confirmation` red in local stress though their product fixes are merged. Diagnosis (harness vs regression) never delivered.                                                                                          | —                              |
| Baseline REF missing                  | `.claude/team/baseline/REF` file not found (was `58658e8`); `baseline.sh --check` needs it re-pinned.                                                                                                                                    | —                              |
| Refactor steps 5–8, 11 + shim removal | Not started; plan in `.claude/team/REFACTOR-plan.md`.                                                                                                                                                                                    | —                              |
