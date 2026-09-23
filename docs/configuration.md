# Configuration

The Rust application works with its defaults. Lua configuration is optional. Set the module path below to `plugin/init.lua` in the local dev-branch checkout.

```lua
local wezterm = require 'wezterm'
local config = wezterm.config_builder()
local vtabs = dofile('/absolute/path/to/wezterm-vertical-tabs/plugin/init.lua')

vtabs.apply_to_config(config, {
  settings = {
    width = 256,
    side = 'left',
    animations = true,
  },
})

return config
```

| Name                  | Value                                                                        |
| --------------------- | ---------------------------------------------------------------------------- |
| `profile`             | Shared catalog/settings scope; default `default`                             |
| `settings`            | Explicit overrides; [generated option reference](options.md)                 |
| `spaces`              | Optional declared catalog of `{id, name, icon?, accent?, rules?, template?}` |
| `templates`           | Dynamic spaces derived from metadata                                         |
| `hooks`               | Optional semantic callbacks                                                  |
| Precedence            | Rust defaults → persisted settings → explicit Lua overrides                  |
| Config-owned settings | Identified in the settings UI; edits do not overwrite explicit Lua values    |
| Actions               | Indexed, relative, negative-index and MRU navigation follow visible tabs     |
| Raw mux identities    | CLI/mux tab IDs retain upstream meaning                                      |
| Sidebar width         | Logical pixels; clamped against available content area                       |
| Side/rail             | `left`/`right`; `expanded`/`collapsed`/`hidden`                              |

**Keyboard and mouse**

| Action                              | macOS                              | Linux / Windows      |
| ----------------------------------- | ---------------------------------- | -------------------- |
| Settings              | Gear tab opens last and keeps its index; later tabs follow it (`Cmd+index`, `Ctrl+Tab`); closes with ×, `Cmd+W`, Escape |
| New tab                             | `Cmd+T`                            | `Ctrl+Shift+T`       |
| Search tabs                         | `Cmd+K`                            | `Ctrl+Shift+K`       |
| Toggle sidebar                      | `Cmd+B`                            | `Ctrl+Shift+B`       |
| Close tab or settings               | `Cmd+W`                            | `Ctrl+Shift+W`       |
| Refresh configuration               | `Cmd+Shift+R`                      | `Ctrl+Shift+R`       |
| Restore closed tab                  | `Cmd+Shift+T`                      | Sidebar context menu |
| Tab 1 through 8 / last              | `Cmd+1..9`                         | `Ctrl+Shift+1..9`    |
| Next / previous tab                 | `Ctrl+Tab` / `Ctrl+Shift+Tab`      | Same                 |
| Previous / next space               | `Ctrl+Alt+Left` / `Ctrl+Alt+Right` | Same                 |
| Search settings                     | `Cmd+F` in settings                | `Ctrl+F` in settings |
| Traverse controls                   | `Tab` / `Shift+Tab`                | Same                 |
| Rename focused tab, folder or space | `F2`                               | Same                 |
| Context menu                        | Right click / `F10`                | Same                 |
| Dismiss menu or page                | `Escape`                           | Same                 |

| Name                  | Value                                                                                                                   |
| --------------------- | ----------------------------------------------------------------------------------------------------------------------- |
| Keyboard preference   | Disable `keyboard_shortcuts` to use custom Lua bindings                                                                 |
| Terminal Control keys | Ordinary Control shortcuts remain available to the shell                                                                |
| Settings              | Own tab with a gear icon and the index after the last tab (`Cmd+index`, `Cmd+9`, `Ctrl+Tab`); close with ×, `Cmd+W`, Escape |
| Search                | Centered palette of sidebar rows; Up/Down select, Left/Right edit the query, Enter opens; matches name, title or index  |
| Search scope          | Current space (indexed), other spaces, filter-hidden, other windows (`window N`), other workspaces, disconnected domains |
| Search `__detached`   | Moves the tab into this window; other workspaces switch; disconnected domains reattach                                  |
| Space row             | Click collapses pinned tabs and folders; hover shows the chevron and the new-folder plus; right click edits the space   |
| Folders               | Create with the space row plus; rename, collapse, reorder, add a tab or ungroup from the context menu                   |
| Tab grouping          | Drag onto a folder, or use the tab context menu                                                                         |
| Reordering            | Drag to a row's edge, where an accent bar marks the landing; folders and spaces drag too; Escape abandons a drag        |
| Split by dragging     | Drop a tab or pane on a row's middle to join it as a split; drag a pane out to an edge or New Tab for its own tab       |
| Ungrouping            | Drop onto New Tab to ungroup and unpin; Remove from folder keeps the pin; processes remain running                      |
| Pinned order          | Pinned tabs and folders precede New Tab; ordinary tabs follow                                                           |
| Space assignment      | Drop a tab on a bottom space control or use Move to space                                                               |
| Folder persistence    | Catalog, names and collapse state persist; live membership follows the session boundary in [Boundaries](limitations.md) |
| Tab title             | Double-click edits in place with the text selected; Enter saves, Escape cancels, an empty title restores the directory  |
| Tab icon              | Machine of the active pane: terminal when local, else its OS, relayed per pane by each patched `wezterm-mux-server`     |
| Tab index             | Replaces the row's icon while hovered, for `Cmd+1`-`Cmd+9` (`show_indexes`)                                             |
| Icons                 | Drawn from WezTerm's bundled symbols font, so their size does not depend on the terminal font's Nerd Font variant       |
| Splits                | Mirrors the layout in one row: side-by-side panes as columns, top/bottom panes as its two lines; click focuses a pane   |
| Close control         | Floats over the hovered tab row without moving its content (`show_close`)                                               |
| Hints                 | Compact, beside the sidebar and level with the control; tabs show none outside the collapsed rail                       |
| Motion                | Finite hover/selection transitions and press shrink; `reduced_motion` disables animation                                |
| Closing tabs          | Idle tabs close at once; only a running process prompts, judged by the pane's owning mux (`confirm_close`, skip list)   |
| Closing splits        | Hovering a split reveals its own ×; the tab's × shows over the icon side of the row                                     |
| Menus and prompts     | Context menus open under the pointer or focused control; confirmations are a dialog with the accepting button selected  |

**Spaces and routing**

```lua
vtabs.apply_to_config(config, {
  spaces = {
    { id = 'home', name = 'Home', icon = '◉' },
    {
      id = 'work', name = 'Work', icon = '⌘', accent = '#89b4fa',
      rules = {
        { fields = { { 'cwd', { '/projects/*', '/work/*' } } } },
        { remote = true, fields = { { 'host', { '*.example.com' } } } },
      },
    },
  },
  templates = {
    {
      id = 'host-$host', name = '$host', icon = '⌁',
      rules = { { remote = true } },
    },
  },
})
```

| Name                      | Value                                                                       |
| ------------------------- | --------------------------------------------------------------------------- |
| Catalog                   | All spaces, including empty spaces; scrollable at the bottom of the sidebar |
| Bottom `+`                | Create a space; distinct from New tab                                       |
| Empty space               | Keeps other tabs running; no automatic shell creation                       |
| Manual assignment         | Tab context menu; Return to auto resumes routing                            |
| Rules                     | A space matches any rule; fields within one rule must all match             |
| Field patterns            | Any pattern within a field; `*` wildcard, otherwise literal                 |
| Fields                    | `domain`, `host`, `user`, `process`, `cwd`, `title`                         |
| Cwd literal               | Also matches descendants separated by `/`                                   |
| `remote`                  | Optional boolean constraint                                                 |
| Templates                 | `$domain`, `$host`, `$user`, `$proc`/`$process`, `$cwd`, `$title`           |
| Deleting a nonempty space | Select a destination for its tabs                                           |
| Private window            | Live tab state stays private; explicit catalog/settings edits remain shared |

**Optional actions**

`vtabs.action(value)` returns a WezTerm callback action. It installs no bindings by itself.

| Value                                                           | Result                                 |
| --------------------------------------------------------------- | -------------------------------------- |
| `'settings'`                                                    | Open settings                          |
| `'create_space'`                                                | Open the create-space form             |
| `'navigator'`                                                   | Open the visible-tab navigator         |
| `'retry_storage'`                                               | Retry a failed durable operation       |
| `{ CreateSpace = { name = 'Work' } }`                           | Create and select a space              |
| `{ SelectSpace = 'work' }`                                      | Select a space by stable ID            |
| `{ RenameSpace = { id = 'work', name = 'Projects' } }`          | Rename a space                         |
| `{ DeleteSpace = { id = 'work', destination = 'home' } }`       | Delete and reassign its tabs           |
| `{ AssignTab = { id = tab_id, space_id = 'work' } }`            | Manually assign a tab                  |
| `{ ReturnToAuto = tab_id }`                                     | Resume automatic routing               |
| `{ PinTab = { id = tab_id, pinned = true } }`                   | Pin a tab                              |
| `{ CreateFolder = { name = 'Project' } }`                       | Create a folder in the selected space  |
| `{ AssignFolder = { tab_id = tab_id, folder_id = folder_id } }` | Group and pin a tab                    |
| `{ NewTabInFolder = folder_id }`                                | Spawn a tab inside a folder            |
| `{ DeleteFolder = folder_id }`                                  | Ungroup tabs without closing them      |
| `{ SetSetting = { key = 'width', value = 300 } }`               | Set an editable preference             |
| `{ SetRail = 'collapsed' }`                                     | Change rail mode                       |
| `'PrivateWindow'`                                               | Create a private window                |
| `'Reopen'`                                                      | Reopen an available launch description |

The full typed action set is `Intent` in `src/core/src/model.rs`. Pane/split actions remain ordinary WezTerm actions.

**Hooks**

```lua
vtabs.apply_to_config(config, {
  hooks = {
    title = function(tab)
      return tab.title
    end,
    routing = function(tab)
      if tab.remote then return 'work' end
      return nil
    end,
    filter = function(tab)
      return true
    end,
    theme = function(context)
      return { accent = '#89b4fa' }
    end,
    footer = function(context)
      return context.profile
    end,
  },
})
```

| Name                  | Value                                                                                            |
| --------------------- | ------------------------------------------------------------------------------------------------ |
| Tab callback input    | Rust tab facts: stable ID, title, cwd, domain, host, user, process, remote and activity metadata |
| Window callback input | `profile`, `private`, `selected_space`, `space`, `active_tab`, `settings`                        |
| `title`               | Display title string                                                                             |
| `routing`             | Existing space ID or `nil`                                                                       |
| `filter`              | Boolean visibility                                                                               |
| `theme`               | Window callback; validated theme-color overrides                                                 |
| `footer`              | Window callback; string; first line shown above spaces, or `nil`                                 |
| Scheduling            | Semantic metadata changes; cached results, no frame-time callbacks                               |
| Stale results         | Discarded after superseding host/model/configuration changes                                     |
| Failure               | Valid state retained; two-second batch deadline; warnings deduplicated per configuration epoch   |

**API**

| Name                                     | Value                                                                        |
| ---------------------------------------- | ---------------------------------------------------------------------------- |
| `wezterm.vtabs.capability`               | Current integration contract marker                                          |
| `wezterm.vtabs.schema`                   | Rust settings schema                                                         |
| `wezterm.vtabs.configure(options)`       | Validated configuration update                                               |
| `wezterm.vtabs.dispatch(window, action)` | Semantic action on one GUI window                                            |
| `wezterm.vtabs.inspect(window)`          | Async diagnostic projection, geometry, model summary and CPU timing counters |

Use [development.md](development.md) for installation, build/update commands and GUI verification. Storage boundaries are documented in [protocol.md](protocol.md).

| Appearance | Default behavior                                                                            |
| ---------- | ------------------------------------------------------------------------------------------- |
| Palette    | Dark blue background `#192231`, accent `#a9c7f5`; saved and explicit colors retain priority |
| Frame      | Sidebar background surrounds rounded terminal content on every edge                         |
| Search     | Compact sidebar trigger; launcher drops down from it, tooltips open below their control     |
| Tab rows   | Index, directory marker and the deepest directory name                                      |
| Directory  | Repository root name, `~/` for the owning machine's home and below it, `/` otherwise        |
| Renames    | Custom title from the context menu or a Lua hook replaces the directory                     |
| Metadata   | Session domain on a second line; `show_metadata`                                            |
| Icons      | Active foreground color; subtle centered hover surface                                      |
| Remote     | OS glyph in its own color; the local terminal glyph keeps the row color (`distro_colors`)   |
| Clipboard  | Cmd/Ctrl+A, C, X and V in editors, including macOS Edit menu equivalents; Ctrl+Shift too    |

```lua
settings = { distro_colors = { arch = '#a6e3a1', remote = '#ffffff' } }
```

| OS ID      | Default   |
| ---------- | --------- |
| `arch`     | `#6eebd3` |
| `ubuntu`   | `#ffa05e` |
| `debian`   | `#ff7ea6` |
| `fedora`   | `#c9a4ff` |
| `nixos`    | `#c3ee7a` |
| `alpine`   | `#eee57a` |
| `centos`   | `#e9a0ff` |
| `rhel`     | `#ff7676` |
| `opensuse` | `#8bf08a` |
| `manjaro`  | `#5ee8a4` |
| `raspbian` | `#ff9ad5` |
| `gentoo`   | `#e0c8ff` |
| `freebsd`  | `#ffb3a6` |
| `linux`    | `#ffcb6b` |
| `macos`    | `#e4e4ea` |
| `windows`  | `#c6f7e2` |
| `remote`   | `#ffe9a8` |
