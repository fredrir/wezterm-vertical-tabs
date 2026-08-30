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
| option                    | default                                                                      | values |
| ------------------------- | ---------------------------------------------------------------------------- | ------ |
| `width`                   | `28`                                                                         | number >= `8` |
| `dim_inactive_panes`      | `false`                                                                      | `true` \| `false` |
| `position`                | `"left"`                                                                     | `"left"` \| `"right"` |
| `collapsed`               | `"rail"`                                                                     | `"rail"` \| `"hidden"` |
| `rail_width`              | `5`                                                                          | number >= `3` |
| `rail_titlebar`           | `"widen"`                                                                    | `"widen"` \| `"band"` \| `"none"` |
| `hide_native_tab_bar`     | `true`                                                                       | `true` \| `false` |
| `poll_ms`                 | `500`                                                                        | number >= `50` |
| `padding`                 | `{ top = 1, left = 2, right = 1 }`                                           | `{ top, left, right }` |
| `edge_to_edge`            | `true`                                                                       | `true` \| `false` |
| `tab_height`              | `"card"`                                                                     | `"card"` (`2`) \| `"row"` (`1`) \| `"tall"` (`3`) |
| `meta`                    | `false`                                                                      | `"auto"` \| `"cwd"` \| `"process"` \| `false` |
| `meta_sep`                | `"  "`                                                                       | string |
| `row_gap`                 | `0`                                                                          | number >= `0` |
| `new_tab_button`          | `"ghost"`                                                                    | `"ghost"` \| `"row"` \| `false` |
| `new_tab_label`           | `"New tab"`                                                                  | string |
| `corners`                 | `"chamfer"`                                                                  | `"chamfer"` \| `"square"` |
| `frame`                   | `false`                                                                      | any |
| `titlebar`                | `"auto"`                                                                     | `"auto"` \| `"integrate"` \| `"plain"` \| `"macos"` |
| `context`                 | `"popover"`                                                                  | `"popover"` \| `false` |
| `popover`                 | `{ width = "auto", follow_pointer = true, fade_ms = 90, overflow = "clip" }` | see below |
| `strip_actions`           | `{ "toggle", "new_tab" }`                                                    | `"toggle"` \| `"new_tab"` \| `"settings"` \| `"search"` \| `{ id, icon, on_click }` |
| `toggle_button`           | `true`                                                                       | `true` \| `false` |
| `close_button`            | `"hover"`                                                                    | `"hover"` \| `"always"` \| `"never"` |
| `confirm_close`           | `true`                                                                       | `true` \| `false` |
| `debug`                   | `false`                                                                      | `true` \| `false` |
| `show_index`              | `false`                                                                      | `true` \| `false` |
| `pinned_style`            | `"dense"`                                                                    | `"dense"` \| `"compact"` \| `"full"` |
| `separator`               | `"gap"`                                                                      | `"rule"` \| `"gap"` \| `"none"` |
| `scroll_indicator`        | `"auto"`                                                                     | `"auto"` \| `"always"` \| `"never"` \| `true` \| `false` |
| `wheel`                   | `"scroll"`                                                                   | `"scroll"` \| `"switch"` |
| `tear_off`                | `true`                                                                       | `true` \| `false` |
| `adopt`                   | `"auto"`                                                                     | `"auto"` \| `true` \| `false` |
| `window_title`            | `true`                                                                       | `true` \| `false` |
| `hover`                   | `"follow"`                                                                   | `"follow"` \| `"press"` |
| `hover_timeout_ms`        | `6000`                                                                       | number >= `0` |
| `tooltip`                 | `"auto"`                                                                     | `"auto"` \| `true` \| `false` |
| `tooltip_delay_ms`        | `600`                                                                        | number >= `0` |
| `double_click_ms`         | `400`                                                                        | number >= `0` |
| `animations`              | `"auto"`                                                                     | `"auto"` \| `true` \| `false` |
| `animation.fps`           | `30`                                                                         | `15`-`60` |
| `animation.expand_ms`     | `220`                                                                        | number >= `0` |
| `animation.collapse_ms`   | `160`                                                                        | number >= `0` |
| `animation.hover`         | `false`                                                                      | `true` \| `false` |
| `ellipsis`                | `"…"`                                                                        | string |
| `icons`                   | `true`                                                                       | `true` \| `false` |
| `icon_map`                | `{}`                                                                         | table |
| `title`                   | `nil`                                                                        | `fun(tab, pane): string` |
| `domain`                  | `"CurrentPaneDomain"`                                                        | string |
| `skip_close_confirmation` | `true`                                                                       | `true` \| `false` |
| `private.env`             | `{ HISTFILE = "", fish_private_mode = "1", VTABS_PRIVATE = "1" }`            | table |
| `keys`                    | `{}`                                                                         | table \| `false` |
| `theme`                   | `{ elevation = 0.06, split = "auto" }`                                       | see Theme |
| `hooks.filter`            | `nil`                                                                        | `fun(tab, mux_window): boolean` |
| `hooks.footer`            | `nil`                                                                        | `fun(mux_window): rows` |
| `hooks.theme`             | `nil`                                                                        | `fun(window, theme): theme` |
| `hooks.route`             | `nil`                                                                        | `fun(meta): space_id` |
| `backend.path`            | `nil`                                                                        | string \| table \| `fun(domain, host)` |
| `backend.repo`            | `"fredrir/wezterm-vertical-tabs"`                                            | string |
| `backend.version`         | plugin version                                                               | string |
| `backend.build`           | `true`                                                                       | `true` \| `false` |
| `backend.uservar`         | `"vtabs"`                                                                    | string |
<!-- options:end -->

| option | note |
| ------ | ---- |
| `width` | re-asserted on the active tab after every resize, one `AdjustPaneSize` at a time so a lagging mux is not overshot; a divider drag is adopted and survives config reloads until `width` itself changes. A window drag corrects on its first frame and again once it stops, never per frame |
| `collapsed` | `"rail"` keeps the pane and narrows it, so the toggle stays reachable |
| `padding` | cells inside the sidebar pane, painted in its own colour; `left = 2` replaces the window padding `edge_to_edge` removes, and mirrors to `right = 2` with `position = "right"`. It is unconditional, so setting `window_padding` yourself leaves one extra cell of air |
| `edge_to_edge` | zeroes wezterm's window padding on the sides the sidebar touches, so its background reaches the window edge; the far side keeps wezterm's `1cell`, and top/bottom are window-global, so the content pane loses its half cell too. It only ever fills in a `window_padding` you left unset, so one set **after** `apply_to_config` wins. The split line stays: pair with `theme = { split = "hidden" }` for a seamless edge |
| `tab_height` | pad rows around the card's content; independent of `meta` |
| `meta` | off by default; opt in with `"auto"`, `"cwd"` or `"process"`. The popover header always shows cwd and domain, whatever this says |
| `rail_width` | raised to the traffic-light reserve on macOS with `INTEGRATED_BUTTONS`: `ceil(70pt / cell width)`, so 9 columns at an 8 pt cell and 6 at 12 pt, the same on a 2x display |
| `rail_titlebar` | `"widen"` grows the collapsed rail to that reserve so the lights land on the sidebar and its toggle stays inside the pane; `"band"` pads the whole window instead, which is the only option once `collapsed = "hidden"` has detached the pane; `"none"` leaves both alone |
| `close_button` | treated as `"always"` when `hover = "press"`, where no background row is ever hovered |
| `context` | `false` removes the mouse trigger only; `m` in keyboard mode still opens the popover |
| `popover.width` | `"auto"` sizes the menu to its widest row, floor 16; a number is taken verbatim. Both are capped at `width - padding.left - padding.right` |
| `popover.follow_pointer` | the pointer selects the row it is over inside the menu; over the scrim the keyboard selection stands |
| `popover.fade_ms` | the menu's own colour fade in, over its rows only; `0` disables it, and nothing fades on close |
| `popover.overflow` | `"clip"` keeps the menu inside the sidebar. `"grow"` is not implemented yet |
| `confirm_close` | the question is a popover level in the sidebar, never wezterm's own overlay, which the release after the click that opened it would dismiss. Asked from a key binding it takes the sidebar pane so `Enter` and `Esc` reach it, and hands focus back on close. The overlay is the fallback only when no sidebar can draw the question |
| `show_index` | renders on the meta line with 2-row cards, inline with `meta = false` |
| `titlebar` | `"macos"` forces the traffic-light reserve on any platform, for previews and screenshots; it changes nothing about the real window buttons |
| `tooltip` | `"auto"` is off in `"press"` mode, where hover never reaches the sidebar |
| `tooltip_delay_ms` | effective delay is `max(tooltip_delay_ms, poll_ms)`; halved in rail mode |
| `animations` | colour only: expand and collapse fade around one hard width snap, never a slide |
| `animation.expand_ms` | the fade in after the resize; the short pre-resize phase keeps its own constant. Same for `collapse_ms` |
| `dim_inactive_panes` | wezterm dims the idle pane by default, which makes the sidebar change shade as focus moves |
| `theme.split` | `"auto"` leaves wezterm's divider; `"hidden"` or a colour repaints **every** split in the window, not just the sidebar's |
| `theme.elevation` | `0.06` = the default tint, `0` = seamless with the terminal background; capped at `0.3`. `theme.content_bg` stays untinted, which is what `frame` paints its gutter with |
| `adopt` | `"auto"` adopts only where this plugin spawns backends; see `docs/limitations.md` |
| `backend.path` | keyed by host or domain; `host` comes from the pane's OSC 7 cwd |

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
| click `✕` / middle click                   | close the tab on release; one that would prompt raises a `Close` / `Cancel` popover first |
| right click                                | context menu on release (overlay in the current pane)                            |
| drag                                       | reorder after 3 rows of travel and 120 ms; dropping across the separator pins/unpins, the list previews the result |
| drag to the inner edge                     | move tab to a new window (edge highlights while armed)                           |
| double-click empty space / click "New Tab" | new tab                                                                          |
| wheel                                      | scroll list (or switch tabs with `wheel = "switch"`)                             |
| click a card's gap row                     | switch to the tab above                                                          |
| drop on a gap row                          | insert below that tab                                                            |
| click the toggle `«`                       | hide the sidebar; `toggle_sidebar` brings it back                                |
| right click a tab                          | action popover, drawn inside the sidebar                                         |
| click away from an open popover            | dismiss without switching tabs; a click level with an item but outside the menu counts as away |
| click a destructive menu item              | runs on release, and only if the release is still on the same item               |
| wheel over an open popover                 | move its selection                                                               |
| move the pointer inside an open popover    | select the row under it (`popover.follow_pointer`)                               |
| footer row                                 | calls the entry's `on_click`                                                     |

| wezterm key                | set to                        | when                                                            |
| -------------------------- | ----------------------------- | --------------------------------------------------------------- |
| `enable_tab_bar`           | `false`                       | `hide_native_tab_bar = true`                                    |
| `pane_focus_follows_mouse` | `true`                        | `hover = "follow"` and you left it unset                        |
| `window_decorations`       | `"INTEGRATED_BUTTONS\|RESIZE"` | macOS, you left it unset, `position = "left"`, `titlebar ~= "plain"` |
| `inactive_pane_hsb`        | identity                      | you left it unset and `dim_inactive_panes = false` (the default)  |
| `window_padding`           | sides touching the sidebar `0`, the far side `"1cell"` | you left it unset and `edge_to_edge = true` (the default) |
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
| `elevation`      | `0.06` — tint `bg` toward `fg`; `0` is seamless with the terminal               |
| `content_bg`     | `resolved_palette.background`, untinted; painted by `frame`                    |
| `title_active`   | `fg` mixed toward `accent`, lifted to 4.5 against `active_bg`; the accent bar returns when it lands within 24 channel units of `fg` |
| `fg`             | `resolved_palette.foreground`                                                  |
| `accent`         | `cursor_bg`, else `tab_bar.active_tab.bg_color`, else `ansi[5]`; each must clear 3.0 against `bg` and 1.2 against `fg` |
| `title_idle`     | `fg` quieted 12% toward `bg`, only when `contrast(fg, bg) >= 5.0`              |
| `meta_fg`        | `fg` mixed 48% toward `bg`, then lifted to 3.5 against `active_bg`             |
| `dim`            | `meta_fg`                                                                      |
| `hover_bg`       | `lift(0.06)`                                                                   |
| `active_bg`      | `lift(0.12)` mixed 12% toward `accent`                                         |
| `focus_bg`       | `bg` mixed 25% toward `accent`                                                 |
| `separator`      | `lift(0.10)`                                                                   |
| `border`         | `lift(0.18)`, lifted to 2.5 against `bg` — popover frame                       |
| `border_idle`    | `lift(0.14)`, lifted to 2.0 against `bg` — idle ghost card                     |
| `ghost_border_hover` | `border` mixed halfway to `accent`, lifted to 2.8 against `bg` — hovered ghost card |
| `new_tab_fg`     | `fg` mixed 30% toward `bg`                                                     |
| `close_fg`       | `fg` mixed 55% toward `bg`, then lifted to 3.0 against `active_bg`             |
| `close_hover_fg` | `ansi[2]`, lifted to 3.0 against `active_bg`                                   |
| `unseen_fg`      | `ansi[4]` when it clears 3.0 against `bg`, else `accent`                       |
| `scroll_fg`      | `lift(0.22)`, lifted to 2.0 against `bg`                                       |
| `scroll_idle_fg` | `scroll_fg` mixed 55% toward `bg`                                              |
| `drag_bg`        | `bg` mixed 35% toward `accent`; a dragged card paints its whole text in `drag_fg` |
| `private_accent` | `ansi[6]`; becomes `accent` for the whole window in a private window           |
| `surface_raised` | `lift(0.09)`, lowered until its text is no harder to read than the body        |
| `scrim`          | fade applied behind an open popover; a contrast target, not a constant         |
| `disabled_fg`    | `meta_fg` mixed 45% toward `surface_raised` — popover items that cannot be chosen |
| `popover_sel_bg` | `accent` — the selected menu row is a fill, not a tint |
| `popover_sel_fg` | `#000000` or `#ffffff`, whichever clears more against `popover_sel_bg`; also paints the row's `▎` marker |
| `popover_sel_hint` | `popover_sel_fg` mixed 40% toward the fill, lifted to 3.0 against it |
| `split`          | `"auto"` | `"hidden"` | a colour; window-global, see the note above           |

Also settable: `active_fg hover_fg pinned_fg drag_fg`. `use_scheme_tab_bar` is deprecated and
ignored — the sidebar paints the terminal background, so there is no background to borrow.

```lua
theme = { accent = "#f5c2e7", active_bg = "#313244", elevation = 0.06 }
```
