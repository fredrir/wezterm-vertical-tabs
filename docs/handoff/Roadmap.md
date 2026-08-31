## Roadmap

### Must Have

| Name                   | Description                                                                                                                                                                                                                                                                                                                                                          | Status  |
| ---------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------- |
| Zen frame fixes        | Fredrir: background wraps the whole terminal, only the main terminal as its own background, thin rounded borders — "important". Needs one-surface colour unification, `frame.inset` composed by the padding owner (not `frame.lua`), Z5/Z6/Z7 security fixes, `colors.split` = card colour. Partial work stranded uncommitted in worktree `agent-aa383cd4d130cce76`. | blocker |
| B7 width adoption      | Per-tab width drift was fredrir's #1 bug; B7 is the last hole: a band-clamped target can be adopted over a user drag. `last_target[wid]` guard designed, not landed.                                                                                                                                                                                                 | blocker |
| Stress XFAIL diagnosis | `split_net` + `close_confirmation` red with fixes merged — harness or regression, unknown. Re-pin `baseline.sh` ref after.                                                                                                                                                                                                                                           | blocker |
| Refactor steps 5–8, 11 | Fredrir: "de-duplicated, more modularized, concerns separated, DRY". Remaining: `store.lua` scopes, split `input.lua`, split `sidebar.lua`, host cleanups + `== nil` list-guard sweep, hit-kind consumers; then shim removal (step 10 tail).                                                                                                                         | planned |

### Should Have

| Name             | Description                                                                                                                                          | Status  |
| ---------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- | ------- |
| Reviewer passes  | Code-quality per refactor step; critique attack on landed Zen; design re-measure strip trio + zen re-verdict; security re-verify Z5/Z6/Z7. None ran. | planned |
| Plan §5.3 / §5.4 | Test-name prefix normalisation; `wezterm_stub.lua` symlink gap.                                                                                      | planned |
| Zen deploy test  | `just deploy`, set `frame = "zen"`, fredrir verdict on default-on.                                                                                   | planned |

### Could Have

| Name                       | Description                                                                | Status  |
| -------------------------- | -------------------------------------------------------------------------- | ------- |
| P4 Spaces                  | Workspaces with routing rules; spec complete at `.claude/team/P4-spec.md`. | planned |
| Fredrir's cut-off "item 4" | Round-2b message ended at "4" — content never delivered; ask.              | blocker |

### Won't Have

| Name                          | Description                                                      | Status  |
| ----------------------------- | ---------------------------------------------------------------- | ------- |
| `·` separators                | Fredrir: "remove all ·".                                         | removed |
| Thick left active-tab border  | Fredrir: remove it.                                              | removed |
| Borders beyond the split line | Fredrir: only sidebar↔editor edge remains.                       | removed |
| `wezterm.time.call_after`     | Leaks a registry entry per call; backend-clocked `anim` instead. | banned  |
