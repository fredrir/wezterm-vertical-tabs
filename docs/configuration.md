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

| option                    | default                                                           | description                                                                                                                                     |
| ------------------------- | ----------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- |
| `width`                   | `28`                                                              | sidebar width in cells (min 8)                                                                                                                  |
| `position`                | `"left"`                                                          | `"left"` or `"right"`                                                                                                                           |
| `hide_native_tab_bar`     | `true`                                                            | sets `enable_tab_bar = false`                                                                                                                   |
| `poll_ms`                 | `500`                                                             | upper bound for `status_update_interval`; drives sidebar refresh                                                                                |
| `padding`                 | `{ top = 1, left = 1, right = 1 }`                                | cells of padding                                                                                                                                |
| `row_gap`                 | `0`                                                               | blank rows between tabs                                                                                                                         |
| `new_tab_button`          | `true`                                                            | show the "New Tab" row                                                                                                                          |
| `new_tab_label`           | `"New Tab"`                                                       | label for that row                                                                                                                              |
| `close_button`            | `"hover"`                                                         | `"hover"` (hovered + active rows), `"always"` or `"never"`; the column is reserved so rows never reflow                                         |
| `confirm_close`           | `true`                                                            | let WezTerm prompt before closing tabs with stateful processes                                                                                  |
| `debug`                   | `false`                                                           | log backend events and hit rows via `wezterm.log_info`                                                                                          |
| `show_index`              | `false`                                                           | prefix titles with the tab index                                                                                                                |
| `pinned_style`            | `"compact"`                                                       | `"compact"`: pin glyph instead of a close button; `"full"`: like normal rows                                                                    |
| `separator`               | `"rule"`                                                          | between pinned and other tabs: `"rule"`, `"gap"` or `"none"`                                                                                    |
| `scroll_indicator`        | `true`                                                            | right-edge thumb when tabs overflow                                                                                                             |
| `wheel`                   | `"scroll"`                                                        | `"scroll"` the list or `"switch"` tabs                                                                                                          |
| `tear_off`                | `true`                                                            | drag a tab onto the sidebar's inner edge (3+ columns of travel) to move it to a new window                                                      |
| `hover_timeout_ms`        | `6000`                                                            | clear hover highlight after inactivity (`0` = never); terminals report no mouse-leave                                                           |
| `double_click_ms`         | `400`                                                             | double-click on empty space opens a new tab                                                                                                     |
| `ellipsis`                | `"…"`                                                             | used when truncating titles                                                                                                                     |
| `icons`                   | `true`                                                            | show process icons (Nerd Font glyphs)                                                                                                           |
| `icon_map`                | `{}`                                                              | process name → glyph overrides; Lua patterns allowed. Also overrides UI glyphs: `close new_tab unseen pinned focus active scroll`               |
| `title`                   | `nil`                                                             | `fun(tab, pane): string` custom title                                                                                                           |
| `domain`                  | `"CurrentPaneDomain"`                                             | domain the sidebar pane is spawned in                                                                                                           |
| `skip_close_confirmation` | `true`                                                            | add `wez-vtabs` to `skip_close_confirmation_for_processes_named`                                                                                |
| `private.env`             | `{ HISTFILE = "", fish_private_mode = "1", VTABS_PRIVATE = "1" }` | env for shells in private windows                                                                                                               |
| `keys`                    | `{}`                                                              | key overrides, see below; `false` disables all defaults                                                                                         |
| `theme`                   | `{ use_scheme_tab_bar = "auto" }`                                 | color overrides, see below                                                                                                                      |
| `hooks.filter`            | `nil`                                                             | `fun(tab, mux_window): boolean` hide tabs from the sidebar (navigation and reordering only touch visible tabs)                                  |
| `hooks.footer`            | `nil`                                                             | `fun(mux_window): (string \| FooterEntry)[]` sticky rows at the bottom; `FooterEntry = { text, fg?, bg?, id?, on_click? = fun(window, entry) }` |
| `hooks.theme`             | `nil`                                                             | `fun(window, theme): theme` per-window theme override                                                                                           |
| `hooks.route`             | `nil`                                                             | reserved for Spaces (`fun(meta): space_id`), not called yet                                                                                     |
| `backend.path`            | `nil`                                                             | path to the `wez-vtabs` binary: string (local only), table keyed by domain (`{ ["local"] = "…", ["host"] = "…" }`) or `fun(domain): string?` |
| `backend.repo`            | `"fredrir/wezterm-vertical-tabs"`                                 | GitHub repo used for release downloads                                                                                                          |
| `backend.version`         | plugin version                                                    | release tag to download (`v<version>`)                                                                                                          |
| `backend.build`           | `true`                                                            | fall back to `cargo build` when no release matches                                                                                              |
| `backend.uservar`         | `"vtabs"`                                                         | user var name used by the backend                                                                                                               |

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
`J`/`K` reorder, `Esc`/`q`/`Ctrl+C` leave. Any other key while the sidebar
is not in keyboard mode returns focus to the content pane.

## Mouse

| gesture                                    | effect                                                                           |
| ------------------------------------------ | -------------------------------------------------------------------------------- |
| left click                                 | switch tab                                                                       |
| click `✕` / middle click                   | close tab                                                                        |
| right click                                | context menu (overlay in the current pane)                                       |
| drag                                       | reorder; dropping across the separator pins/unpins, the list previews the result |
| drag to the inner edge                     | move tab to a new window (edge highlights while armed)                           |
| double-click empty space / click "New Tab" | new tab                                                                          |
| wheel                                      | scroll list (or switch tabs with `wheel = "switch"`)                             |
| footer row                                 | calls the entry's `on_click`                                                     |


## Public API

`vtabs.apply_to_config(config, opts)`, `vtabs.action.*`,
`vtabs.toggle_sidebar(window)`, `vtabs.show_sidebar(window, bool)`,
`vtabs.sync(window, { force = true })`, `vtabs.invalidate_theme(window_id?)`,
`vtabs.is_sidebar_pane(pane)`, `vtabs.is_private_window(window)`, `vtabs.version`.

## Theme

`bg fg dim accent active_bg active_fg hover_bg hover_fg focus_bg pinned_fg
separator new_tab_fg close_fg close_hover_fg unseen_fg private_accent drag_bg
drag_fg scroll_fg`

```lua
theme = { accent = "#f5c2e7", active_bg = "#313244" }
```
