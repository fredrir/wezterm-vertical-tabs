# Configuration

## Install

```lua
local wezterm = require "wezterm"
local config = wezterm.config_builder()

-- your own bindings first: the plugin never overrides a key you already bound
config.keys = { ... }

local vtabs = wezterm.plugin.require "https://github.com/fredrir/wezterm-vertical-tabs"
vtabs.apply_to_config(config, {
  width = 28,
})
return config
```

## Options

<!-- options:start -->
| option                    | default                                                           | description |
| ------------------------- | ----------------------------------------------------------------- | ----------- |
| `width`                   | `28`                                                              | sidebar width in cells (min 8); re-asserted on the active tab after every window resize, a divider drag is adopted until the config reloads. Two-row cards give a 19-column title and a 20-column meta line at 28; raise to `32` if titles truncate too often |
| `position`                | `"left"`                                                          | `"left"` or `"right"` |
| `hide_native_tab_bar`     | `true`                                                            | sets `enable_tab_bar = false` |
| `poll_ms`                 | `500`                                                             | upper bound for `status_update_interval`; drives sidebar refresh |
| `padding`                 | `{ top = 1, left = 1, right = 1 }`                                | cells of padding; `top` is added to the top strip, which owns the rows above the first card |
| `tab_height`              | `"card"`                                                          | `"card"` (or `2`): 2 painted rows per tab; `"row"` (or `1`): 1 row, same as `meta = false` |
| `meta`                    | `"auto"`                                                          | second card row: `"auto"` (cwd for shells, `user@host` for ssh, `proc · dir` otherwise, `domain · cwd` on a mux), `"cwd"`, `"process"`, or `false` for 1-row cards |
| `row_gap`                 | `1`                                                               | blank rows after each card; the gap row is part of the card's click target |
| `new_tab_button`          | `"ghost"`                                                         | `"ghost"`: dashed card, sticky at the bottom; `"row"`: single row; `false`: hidden. `true` = `"ghost"` |
| `new_tab_label`           | `"New tab"`                                                       | label inside the card |
| `corners`                 | `"chamfer"`                                                       | `"chamfer"`: quadrant-cut card corners; `"square"`. Forced to `"square"` when `custom_block_glyphs = false` |
| `titlebar`                | `"auto"`                                                          | `"auto"`: reserve cells for the macOS traffic lights when the window has `INTEGRATED_BUTTONS`; `"integrate"`: always reserve; `"plain"`: never |
| `toggle_button`           | `true`                                                            | draw `«`/`»` in the top strip; clicking it hides the sidebar, `toggle_sidebar` brings it back |
| `close_button`            | `"hover"`                                                         | `"hover"` (hovered + active rows), `"always"` or `"never"`; the column is reserved so rows never reflow. The hit target is 3 columns × both card rows. Treated as `"always"` when `hover = "press"`, where no non-active row is ever hovered |
| `confirm_close`           | `true`                                                            | let WezTerm prompt before closing tabs with stateful processes |
| `debug`                   | `false`                                                           | log backend events and hit rows via `wezterm.log_info` |
| `show_index`              | `false`                                                           | prefix titles with the tab index; with two-row cards the index renders on the meta line (`1 · ~/projects/api`) so the title grid never shifts, and goes back inline with `meta = false` |
| `pinned_style`            | `"dense"`                                                         | `"dense"`: 1-row entries, pin glyph on hover; `"compact"`: as before; `"full"`: normal 2-row cards |
| `separator`               | `"gap"`                                                           | between pinned and other tabs: `"gap"`, `"rule"` or `"none"` |
| `scroll_indicator`        | `"auto"`                                                          | right-edge thumb when tabs overflow: `"auto"` dims it while the sidebar is idle, `"always"`, `"never"`. `true`/`false` accepted |
| `wheel`                   | `"scroll"`                                                        | `"scroll"` the list or `"switch"` tabs |
| `tear_off`                | `true`                                                            | drag a tab onto the sidebar's inner edge (3+ columns of travel) to move it to a new window |
| `adopt`                   | `"auto"`                                                          | take over an unmapped pane that carries the `wez-vtabs:` title marker instead of splitting a second sidebar. `"auto"`: only in a domain this plugin spawns backends in (local, already-spawned, or one `backend.path` resolves); `true`: any domain; `false`: never. See the identity table in `docs/limitations.md` |
| `window_title`            | `true`                                                            | while the sidebar is the active pane, title the window after the content pane instead. `false` leaves `format-window-title` unregistered |
| `hover`                   | `"follow"`                                                        | `"follow"`: the sidebar is the tab's active pane while the pointer is over it (sets `pane_focus_follows_mouse = true` when you left it unset — this is a global wezterm option); `"press"`: only from press to release |
| `hover_timeout_ms`        | `6000`                                                            | clear hover highlight after inactivity (`0` = never); terminals report no mouse-leave |
| `double_click_ms`         | `400`                                                             | double-click on empty space opens a new tab |
| `ellipsis`                | `"…"`                                                             | used when truncating titles |
| `icons`                   | `true`                                                            | show process icons (Nerd Font glyphs) |
| `icon_map`                | `{}`                                                              | process name → glyph overrides; Lua patterns allowed. Also overrides UI glyphs: `close new_tab unseen pinned focus active scroll` |
| `title`                   | `nil`                                                             | `fun(tab, pane): string` custom title |
| `domain`                  | `"CurrentPaneDomain"`                                             | domain the sidebar pane is spawned in |
| `skip_close_confirmation` | `true`                                                            | add `wez-vtabs` to `skip_close_confirmation_for_processes_named` |
| `private.env`             | `{ HISTFILE = "", fish_private_mode = "1", VTABS_PRIVATE = "1" }` | env for shells in private windows |
| `keys`                    | `{}`                                                              | key overrides, see below; `false` disables all defaults |
| `theme`                   | `{ elevation = 0 }`                                               | color overrides, see below |
| `hooks.filter`            | `nil`                                                             | `fun(tab, mux_window): boolean` hide tabs from the sidebar (navigation and reordering only touch visible tabs) |
| `hooks.footer`            | `nil`                                                             | `fun(mux_window): (string \| FooterEntry)[]` sticky rows at the bottom; `FooterEntry = { text, fg?, bg?, id?, on_click? = fun(window, entry) }` |
| `hooks.theme`             | `nil`                                                             | `fun(window, theme): theme` per-window theme override |
| `hooks.route`             | `nil`                                                             | reserved for Spaces (`fun(meta): space_id`), not called yet |
| `backend.path`            | `nil`                                                             | path to the `wez-vtabs` binary: string (this machine), table keyed by host or domain (`{ ["local"] = "…", archie = "…" }`) or `fun(domain, host): string?`; `host` comes from the pane\'s OSC 7 cwd, which is what identifies panes proxied through a mux server |
| `backend.repo`            | `"fredrir/wezterm-vertical-tabs"`                                 | GitHub repo used for release downloads |
| `backend.version`         | plugin version                                                    | release tag to download (`v<version>`) |
| `backend.build`           | `true`                                                            | fall back to `cargo build` when no release matches |
| `backend.uservar`         | `"vtabs"`                                                         | user var name used by the backend |
<!-- options:end -->

## Keys

| name                                   | macOS                         | Linux / Windows              | action                          |
| -------------------------------------- | ----------------------------- | ---------------------------- | ------------------------------- |
| `toggle_sidebar`                       | `CMD+b`                       | `CTRL+SHIFT+b`               | show/hide sidebar               |
| `focus_sidebar`                        | `CMD+SHIFT+b`                 | `CTRL+SHIFT+ALT+b`           | keyboard mode in the sidebar    |
| `new_tab`                              | `CMD+t`                       | `CTRL+SHIFT+t`               | new tab (inherits domain + cwd) |
| `close_tab`                            | `CMD+w`                       | `CTRL+SHIFT+w`               | close current tab               |
| `new_window`                           | `CMD+n`                       | `CTRL+SHIFT+n`               | new window                      |
| `reopen_closed`                        | `CMD+SHIFT+t`                 | `CTRL+SHIFT+ALT+t`           | reopen last closed tab          |
| `pin_tab`                              | `CMD+SHIFT+d`                 | `CTRL+SHIFT+ALT+d`           | pin/unpin current tab           |
| `private_window`, `private_window_alt` | `CMD+SHIFT+p`, `CMD+SHIFT+n`  | `CTRL+SHIFT+ALT+p`, `+n`     | new private window              |
| `next_tab` / `prev_tab`                | `CTRL+Tab` / `CTRL+SHIFT+Tab` | same                         | cycle visible tabs              |
| `next_tab_alt` / `prev_tab_alt`        | `CMD+SHIFT+]` / `[`           | `CTRL+SHIFT+ALT+]` / `[`     | cycle                           |
| `next_tab_arrow` / `prev_tab_arrow`    | `CMD+OPT+Right` / `Left`      | —                            | cycle                           |
| `move_tab_up` / `move_tab_down`        | `CMD+SHIFT+PageUp/Down`       | `CTRL+SHIFT+ALT+PageUp/Down` | reorder                         |
| `tab_1` … `tab_8`, `tab_last`          | `CMD+1` … `CMD+9`             | `CTRL+SHIFT+1` … `9`         | jump to visible tab             |

```lua
keys = {
  new_tab = false,                             -- keep your own binding
  toggle_sidebar = { key = "b", mods = "ALT" },
}
```

manual binding:

```lua
{ key = "b", mods = "CMD", action = vtabs.action.toggle_sidebar }
{ key = "h", mods = "CMD", action = vtabs.action.activate_pane_direction "Left" } -- skips the sidebar
{ key = "1", mods = "CMD", action = vtabs.action.activate_tab(0) }              -- 0-based, -1 = last
```

Available: `toggle_sidebar focus_sidebar new_tab close_tab reopen_closed pin_tab
private_window new_window tear_off rename_tab next_tab prev_tab move_tab_up
move_tab_down activate_tab(index) activate_pane_direction(dir)`.

### Sidebar keyboard mode

`focus_sidebar` moves focus into the sidebar: `j`/`k`/arrows/`Tab`/`Shift+Tab`
move, `g`/`G`/`Home`/`End` jump, `1`–`9` switch directly, `Enter`/`Space`
switch, `x`/`d`/`Delete` close, `p` pin, `n` new tab, `r` rename, `m` menu,
`J`/`K` reorder, `Esc`/`q`/`Ctrl+C` leave.

Keys that reach the sidebar while it is *not* in keyboard mode are forwarded to
the tab's content pane, which then takes focus back.

| guard                | value                                                                 |
| -------------------- | ---------------------------------------------------------------------- |
| source               | a sidebar pane this process authenticated, in the window's active tab   |
| target               | that same tab's content pane, same domain, not an overlay              |
| payload              | structurally one key press: a single UTF-8 codepoint, or one `ESC`-prefixed sequence (CSI / SS3 / alt-key), <= 16 bytes, never a paste bracket |
| rate                 | token bucket per source pane: 20 burst, 60/s sustained; over budget the key is dropped but focus still moves |
| paste                | a `paste` event is decoded and delivered whole with `pane:paste`, <= 64 KiB |

## Mouse

| gesture                                    | effect                                                                           |
| ------------------------------------------ | -------------------------------------------------------------------------------- |
| left click                                 | switch tab                                                                       |
| click `✕` / middle click                   | close tab                                                                        |
| right click                                | context menu on release (overlay in the current pane)                            |
| drag                                       | reorder after 3 rows of travel and 120 ms; dropping across the separator pins/unpins, the list previews the result |
| drag to the inner edge                     | move tab to a new window (edge highlights while armed)                           |
| double-click empty space / click "New Tab" | new tab                                                                          |
| wheel                                      | scroll list (or switch tabs with `wheel = "switch"`)                             |
| click a card's gap row                     | switch to the tab above                                                          |
| drop on a gap row                          | insert below that tab                                                            |
| click the toggle `«`                       | hide the sidebar; `toggle_sidebar` brings it back                                |
| footer row                                 | calls the entry's `on_click`                                                     |

| wezterm key                | set to                        | when                                                            |
| -------------------------- | ----------------------------- | --------------------------------------------------------------- |
| `enable_tab_bar`           | `false`                       | `hide_native_tab_bar = true`                                    |
| `pane_focus_follows_mouse` | `true`                        | `hover = "follow"` and you left it unset                        |
| `window_decorations`       | `"INTEGRATED_BUTTONS\|RESIZE"` | macOS, you left it unset, `position = "left"`, `titlebar ~= "plain"` |
| `status_update_interval`   | `min(yours, poll_ms)`         | always                                                          |

`window_decorations = "RESIZE"` alone hides the macOS window buttons and pins the window in
place; the plugin never sets it and warns once if you do.

| focus                | behaviour                                                                                  |
| -------------------- | ------------------------------------------------------------------------------------------ |
| `hover = "follow"`   | pointer over the sidebar = sidebar focused; keys that are not sidebar bindings are forwarded to the tab's content pane, which then takes focus back |
| `hover = "press"`    | the sidebar holds focus from press to release only                                          |
| press                | never hands focus to the content pane, so the drag and the release reach the sidebar        |
| window title         | while the sidebar is the active pane the plugin titles the window after the content pane; a `format-window-title` handler you register *before* `apply_to_config` wins, one registered after it never runs |


## Public API

`vtabs.apply_to_config(config, opts)`, `vtabs.action.*`,
`vtabs.toggle_sidebar(window)`, `vtabs.show_sidebar(window, bool)`,
`vtabs.sync(window, { force = true })`, `vtabs.invalidate_theme(window_id?)`,
`vtabs.is_sidebar_pane(pane)` (true for any pane presenting as a sidebar backend; for skipping, never for trust), `vtabs.window_title(tab, pane, tabs, panes)`, `vtabs.is_private_window(window)`, `vtabs.version`.

WezTerm runs only the first `format-window-title` handler; register yours before
`apply_to_config` or call `vtabs.window_title` from it.

## Theme

`lift(t)` = `bg` mixed `t` toward `fg`, halved on light schemes.

| key              | default                                                                        |
| ---------------- | ------------------------------------------------------------------------------ |
| `bg`             | `resolved_palette.background`                                                  |
| `elevation`      | `0` — tint `bg` toward `fg`; `0.06` is the pre-P1 raised sidebar                |
| `fg`             | `resolved_palette.foreground`                                                  |
| `accent`         | `cursor_bg`, else `tab_bar.active_tab.bg_color`, else `ansi[5]`; each must clear 3.0 against `bg` and 1.2 against `fg` |
| `title_idle`     | `fg` quieted 12% toward `bg`, only when `contrast(fg, bg) >= 5.0`              |
| `meta_fg`        | `fg` mixed 48% toward `bg`, then lifted to 3.5 against `active_bg`             |
| `dim`            | `meta_fg`                                                                      |
| `hover_bg`       | `lift(0.06)`                                                                   |
| `active_bg`      | `lift(0.12)` mixed 12% toward `accent`                                         |
| `focus_bg`       | `bg` mixed 25% toward `accent`                                                 |
| `separator`      | `lift(0.10)`                                                                   |
| `border`         | `lift(0.18)`, lifted to 2.5 against `bg` — hovered ghost card                  |
| `border_idle`    | `lift(0.14)`, lifted to 2.0 against `bg` — dashed ghost card                   |
| `new_tab_fg`     | `fg` mixed 30% toward `bg`                                                     |
| `close_fg`       | `fg` mixed 55% toward `bg`, then lifted to 3.0 against `active_bg`             |
| `close_hover_fg` | `ansi[2]`, lifted to 3.0 against `active_bg`                                   |
| `unseen_fg`      | `ansi[4]` when it clears 3.0 against `bg`, else `accent`                       |
| `scroll_fg`      | `lift(0.22)`, lifted to 2.0 against `bg`                                       |
| `scroll_idle_fg` | `scroll_fg` mixed 55% toward `bg`                                              |
| `drag_bg`        | `bg` mixed 35% toward `accent`; a dragged card paints its whole text in `drag_fg` |
| `private_accent` | `ansi[6]`; becomes `accent` for the whole window in a private window           |

Also settable: `active_fg hover_fg pinned_fg drag_fg`. `use_scheme_tab_bar` is deprecated and
ignored — the sidebar paints the terminal background, so there is no background to borrow.

```lua
theme = { accent = "#f5c2e7", active_bg = "#313244", elevation = 0.06 }
```
