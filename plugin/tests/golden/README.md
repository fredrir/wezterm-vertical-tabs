# Golden frames

The equivalence oracle. Every scene here is rendered by the Rust renderer — no display — and
committed so a diff is falsifiable.

## Commands

| Do this | Run |
|---|---|
| Check everything against the goldens | `cd backend && cargo test -p vtabs-view` |
| Check as part of the full gate | `just check` |
| Re-render one-off, by hand | `cd backend && cargo run -q -p wez-vtabs -- dump-frames ../plugin/tests/golden/scenes /tmp/frames` |
| Re-pin after a reviewed, intentional visual change | same as above into `plugin/tests/golden/frames`, then `git diff` and commit |

`vtabs-view/tests/golden_parity.rs` (sidebar scenes) and `settings_parity.rs` (`settings-*`) render
every scene under `scenes/` and byte-diff both dumps against this directory; a mismatch fails with
the diff and nothing here is touched.

## Scenes

`scenes/<scene>.json` is the renderer's input for each golden: the resolved theme and glyphs, the
pane size, and either a sidebar `RenderInput` (`vtabs-view/src/scene.rs`) or, for `settings-*`, the
settings model plus the local ui state. They are hand-maintained fixtures now that the Lua
renderer that first produced them is gone.

## Layout

| File | Content |
|---|---|
| `<scene>.txt` | text dump: a 2-row column ruler, then one line per frame row, ANSI stripped |
| `<scene>.styled.txt` | styled dump: one line per frame row, no ruler — row *N* here is row *N* of `<scene>.txt` minus its 2-row ruler (i.e. `<scene>.txt` line `N+2`) |

## Styled format

One row per line: `<row>: <span> <span> ...`, left-to-right, space-separated.

| Span | Meaning |
|---|---|
| `#fg/#bg:width` | a run of `width` cells sharing one fg/bg pair |
| `#fg/#bg*:width` | same, bold |
| `#000000/#bg:width` | a run with no visible glyph (pure whitespace) — the renderer omits the fg SGR code here to save bytes, so `#000000` is a fixed placeholder, not a real colour |

Adjacent cells are merged into one span exactly where the renderer's own ANSI encoder merges
them (`vtabs/render.lua`'s `emit()`), so span boundaries reflect real SGR-run boundaries, not an
artifact of the dump. Width is display-cell width (`vtabs.util.width`), not byte or codepoint
count, so a double-wide glyph still costs 2.

Both dumps are parsed from the same raw ANSI (CUP + SGR truecolor) the renderer emits — the
styled dump extends `support/helpers.lua`'s ANSI-stripping (`M.strip`) instead of discarding the
SGR it finds.

## Scene inventory

| Scene | Pins |
|---|---|
| `tabs` | the baseline card list: pinned, active, unseen |
| `popover-open` | context menu open over a card |
| `popover-confirm` | the confirm sub-level of a destructive item: danger row + selected Cancel |
| `tall` | `tab_height = "tall"` |
| `frame` | `cfg.frame` (margin/corners/tint) card framing |
| `active-unseen` | active tab that also carries the unseen marker |
| `rail-5`, `rail-9` | rail (icon-only) mode at two narrow widths |
| `collapsed` | rail mode, hovered — today's "detach" name, now a real frame instead of a sentence |
| `hover` | pointer over a card body |
| `hover-close` | pointer over the ✕ glyph — identical text to `hover`, colour-only (`close_hover_fg`) |
| `new-tab-hover` | pointer over the ghost "new tab" row |
| `drag` | a card mid-drag over another slot |
| `drag-outside` | same drag, past the tear-off threshold — identical text to `drag`, colour-only (accent edge column) |
| `private` | a private-window tab |
| `strip-macos` | the macOS integrated-button strip geometry |
| `overflow` | more tabs than rows, scrolled |
| `settings-100`, `settings-60`, `settings-40` | the settings screen at preview / no-preview / too-narrow widths |
| `settings-pending` | settings screen with an uncommitted edit — the live preview must reflect `pending`, not the saved config |
