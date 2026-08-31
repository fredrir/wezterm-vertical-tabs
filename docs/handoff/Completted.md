## Work Completed

### Changes Made

| Description | Type |
| --- | --- |
| P0 batch: per-tab width drift, sidebar duplication after restart (adoption 332ms vs 12s), tabs jumping on click, theme defaults, right-click blanking, new-tab flash — all fixed and pinned by tests. | requested |
| Visual overhaul: 3-row centred cards, ghost New-tab card with dashed edges, strip with toggle, no `·` separators, no thick active-tab bar, bigger `nf-md-close_thick` ✕. | requested |
| Tinted sidebar restored at `elevation = 0.06`; only remaining visible edge is the sidebar↔editor split line. | requested |
| In-sidebar popover replaces all WezTerm overlays (InputSelector/PromptInputLine/close-confirm); close confirm works on press, not hold. | requested |
| Settings page: full TUI as a tab (`--role settings`), descriptor-driven form from `schema.lua`, LOCKED badges where config-as-code wins, live preview, `settings.json` 0600. | requested |
| Zen frame: generated PNG `config.background` layer with rounded card (`frame = "zen"`, default false pending fredrir's verdict). | requested |
| Refactor steps 1–4, 9, 10 of 12 (`.claude/team/REFACTOR-plan.md`): `mux.lua` façade, single action-dispatch table (fixed the inert default ⚙ button — only intended behaviour change), layout derives strip defaults, hit-kind constants, test suite split into `run_<module>.lua` + `tests/support/helpers.lua`. | requested |
| Baseline harness gating the refactor: `scripts/baseline.sh --pin/--check --only <kinds>`; frames byte-compared against pinned ref. | implicit |
| Sidebar trust model: adoption only after echoing a process-minted token; marker nonce ≠ auth token; kill via pinned `gui-sock-<pid>`. | implicit |
| Option docs generated from `schema.lua` via `scripts/gen-docs.lua`; `just check` fails on drift. | conceptual |
