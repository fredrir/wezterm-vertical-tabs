# Configuration


## Options

| option                    | default                                                           | description                                                                                                                                              |
| ------------------------- | ----------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `width`                   | `28`                                                              | sidebar width in cells (min 8)                                                                                                                           |
| `position`                | `"left"`                                                          | `"left"` or `"right"`                                                                                                                                    |
| `hide_native_tab_bar`     | `true`                                                            | sets `enable_tab_bar = false`                                                                                                                            |
| `poll_ms`                 | `500`                                                             | upper bound for `status_update_interval`; drives sidebar refresh                                                                                         |
| `padding`                 | `{ top = 1, left = 1, right = 1 }`                                | cells of padding                                                                                                                                         |
| `row_gap`                 | `0`                                                               | blank rows between tabs                                                                                                                                  |
| `new_tab_button`          | `true`                                                            | show the "New Tab" row                                                                                                                                   |
| `new_tab_label`           | `"New Tab"`                                                       | label for that row                                                                                                                                       |
| `close_button`            | `"hover"`                                                         | `"hover"`, `"always"` or `"never"`                                                                                                                       |
| `confirm_close`           | `true`                                                            | ask before closing tabs with stateful processes                                                                                                          |
| `debug`                   | `false`                                                           | log backend events and hit rows via `wezterm.log_info`                                                                                                   |
| `show_index`              | `false`                                                           | prefix titles with the tab index                                                                                                                         |
| `pinned_style`            | `"compact"`                                                       | `"compact"` hides close buttons on pinned tabs                                                                                                           |
| `wheel`                   | `"scroll"`                                                        | `"scroll"` the list or `"switch"` tabs                                                                                                                   |
| `tear_off`                | `"edge"`                                                          | `"edge"`: drop on the outer sidebar edge to move a tab to a new window; `"outside"`: only beyond the pane (needs WezTerm to report it); `false` disables |
| `hover_timeout_ms`        | `2000`                                                            | clear hover highlight after inactivity (`0` = never)                                                                                                     |
| `double_click_ms`         | `400`                                                             | double-click on empty space opens a new tab                                                                                                              |
| `ellipsis`                | `"…"`                                                             | used when truncating titles                                                                                                                              |
| `icons`                   | `true`                                                            | show process icons (Nerd Font glyphs)                                                                                                                    |
| `icon_map`                | `{}`                                                              | process name → glyph overrides; Lua patterns allowed                                                                                                     |
| `title`                   | `nil`                                                             | `fun(tab, pane): string` custom title                                                                                                                    |
| `domain`                  | `"CurrentPaneDomain"`                                             | domain the sidebar pane is spawned in                                                                                                                    |
| `skip_close_confirmation` | `true`                                                            | add `wez-vtabs` to `skip_close_confirmation_for_processes_named`                                                                                         |
| `private.env`             | `{ HISTFILE = "", fish_private_mode = "1", VTABS_PRIVATE = "1" }` | env for shells in private windows                                                                                                                        |
| `keys`                    | `{}`                                                              | key overrides, see below; `false` disables all defaults                                                                                                  |
| `theme`                   | `{}`                                                              | color overrides, see below                                                                                                                               |
| `hooks.filter`            | `nil`                                                             | `fun(tab, mux_window): boolean` hide tabs from the sidebar                                                                                               |
| `hooks.footer`            | `nil`                                                             | `fun(mux_window): string[]` extra rows under the list                                                                                                    |
| `backend.path`            | `nil`                                                             | explicit path to the `wez-vtabs` binary                                                                                                                  |
| `backend.repo`            | `"fredrir/wez-vertical-tabs"`                                     | GitHub repo used for release downloads                                                                                                                   |
| `backend.version`         | plugin version                                                    | release tag to download (`v<version>`)                                                                                                                   |
| `backend.build`           | `true`                                                            | fall back to `cargo build` when no release matches                                                                                                       |
| `backend.uservar`         | `"vtabs"`                                                         | user var name used by the backend                                                                                                                        |

## Keys

Defaults use `CMD` on macOS and `CTRL` elsewhere (`SUPER` below).

| name                            | default                       | action                          |
| ------------------------------- | ----------------------------- | ------------------------------- |
| `toggle_sidebar`                | `SUPER+b`                     | show/hide sidebar               |
| `focus_sidebar`                 | `SUPER+SHIFT+b`               | keyboard mode in the sidebar    |
| `new_tab`                       | `SUPER+t`                     | new tab (inherits domain + cwd) |
| `close_tab`                     | `SUPER+w`                     | close current tab               |
| `reopen_closed`                 | `SUPER+SHIFT+t`               | reopen last closed tab          |
| `pin_tab`                       | `SUPER+SHIFT+p`               | pin/unpin current tab           |
| `private_window`                | `SUPER+SHIFT+n`               | new private window              |
| `new_window`                    | `SUPER+n`                     | new window                      |
| `next_tab` / `prev_tab`         | `CTRL+Tab` / `CTRL+SHIFT+Tab` | cycle tabs                      |
| `move_tab_up` / `move_tab_down` | `SUPER+SHIFT+PageUp/PageDown` | reorder                         |
| `tab_1` … `tab_8`, `tab_last`   | `SUPER+1` … `SUPER+9`         | jump to tab                     |

```lua
keys = {
  new_tab = false,                             -- keep your own binding
  toggle_sidebar = { key = "b", mods = "ALT" },
}
```

Every action is also exposed for manual binding:

```lua
{ key = "b", mods = "CMD", action = vtabs.action.toggle_sidebar }
{ key = "h", mods = "CMD", action = vtabs.action.activate_pane_direction "Left" } -- skips the sidebar
```

Available: `toggle_sidebar focus_sidebar new_tab close_tab reopen_closed pin_tab
private_window new_window tear_off rename_tab next_tab prev_tab move_tab_up
move_tab_down activate_tab(index) activate_pane_direction(dir)`.

### Sidebar keyboard mode

`focus_sidebar` moves focus into the sidebar: `j/k` or arrows move, `Enter`
switches, `x` closes, `p` pins, `n` new tab, `r` rename, `m` menu, `J/K`
reorder, `Esc`/`q` leaves.

## Mouse

| gesture                                    | effect                                               |
| ------------------------------------------ | ---------------------------------------------------- |
| left click                                 | switch tab                                           |
| click `✕` / middle click                   | close tab                                            |
| right click                                | context menu                                         |
| drag                                       | reorder; dropping across the separator pins/unpins   |
| drag to the outer edge                     | move tab to a new window                             |
| double-click empty space / click "New Tab" | new tab                                              |
| wheel                                      | scroll list (or switch tabs with `wheel = "switch"`) |

## Theme

Colors default to the window's color scheme (`resolved_palette`, including
`tab_bar` colors when set). Override any of:

`bg fg dim accent active_bg active_fg hover_bg hover_fg pinned_fg separator
new_tab_fg close_fg close_hover_fg unseen_fg private_accent drag_bg drag_fg
focus_bg`

```lua
theme = { accent = "#f5c2e7", active_bg = "#313244" }
```
