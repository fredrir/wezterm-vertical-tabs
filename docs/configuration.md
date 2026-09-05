# Configuration

The Rust application works with its defaults. Lua configuration is optional.

```lua
local wezterm = require 'wezterm'
local config = wezterm.config_builder()
local vtabs = wezterm.plugin.require 'https://github.com/fredrir/wezterm-vertical-tabs'

vtabs.apply_to_config(config, {
  settings = {
    width = 256,
    side = 'left',
    animations = true,
  },
})

config.keys = {}
for i = 1, 9 do
  table.insert(config.keys, {
    key = tostring(i), mods = 'SUPER',
    action = wezterm.action.ActivateTab(i - 1),
  })
end

-- Optional project actions.
table.insert(config.keys, {
  key = ',', mods = 'SUPER', action = vtabs.action('settings'),
})
return config
```

| Name | Value |
| --- | --- |
| `profile` | Shared catalog/settings scope; default `default` |
| `settings` | Explicit overrides; [generated option reference](options.md) |
| `spaces` | Optional declared catalog of `{id, name, icon?, accent?, rules?, template?}` |
| `templates` | Dynamic spaces derived from metadata |
| `hooks` | Optional semantic callbacks |
| Precedence | Rust defaults → persisted settings → explicit Lua overrides |
| Config-owned settings | Identified in the settings UI; edits do not overwrite explicit Lua values |
| Native actions | Indexed, relative, negative-index and MRU navigation follow visible tabs |
| Raw mux identities | CLI/mux tab IDs retain upstream meaning |
| Sidebar width | Logical pixels; clamped against available native content area |
| Side/rail | `left`/`right`; `expanded`/`collapsed`/`hidden` |

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

| Name | Value |
| --- | --- |
| Catalog | All spaces, including empty spaces; scrollable with an accessible `(+)` |
| `(+)` | Create a space; distinct from New tab |
| Empty space | Keeps other tabs running; no automatic shell creation |
| Manual assignment | Tab context menu; Return to auto resumes routing |
| Rules | A space matches any rule; fields within one rule must all match |
| Field patterns | Any pattern within a field; `*` wildcard, otherwise literal |
| Fields | `domain`, `host`, `user`, `process`, `cwd`, `title` |
| Cwd literal | Also matches descendants separated by `/` |
| `remote` | Optional boolean constraint |
| Templates | `$domain`, `$host`, `$user`, `$proc`/`$process`, `$cwd`, `$title` |
| Deleting a nonempty space | Select a destination for its tabs |
| Private window | Live tab state stays private; explicit catalog/settings edits remain shared |

**Optional actions**

`vtabs.action(value)` returns a WezTerm callback action. It installs no bindings by itself.

| Value | Result |
| --- | --- |
| `'settings'` | Open settings |
| `'create_space'` | Open the create-space form |
| `'navigator'` | Open the visible-tab navigator |
| `'retry_storage'` | Retry a failed durable operation |
| `{ CreateSpace = { name = 'Work' } }` | Create and select a space |
| `{ SelectSpace = 'work' }` | Select a space by stable ID |
| `{ RenameSpace = { id = 'work', name = 'Projects' } }` | Rename a space |
| `{ DeleteSpace = { id = 'work', destination = 'home' } }` | Delete and reassign its tabs |
| `{ AssignTab = { id = tab_id, space_id = 'work' } }` | Manually assign a tab |
| `{ ReturnToAuto = tab_id }` | Resume automatic routing |
| `{ PinTab = { id = tab_id, pinned = true } }` | Pin a tab |
| `{ SetSetting = { key = 'width', value = 300 } }` | Set an editable preference |
| `{ SetRail = 'collapsed' }` | Change rail mode |
| `'PrivateWindow'` | Create a private window |
| `'Reopen'` | Reopen an available launch description |

The full typed action set is `Intent` in `crates/vtabs-core/src/model.rs`. Native pane/split actions remain ordinary WezTerm actions.

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

| Name | Value |
| --- | --- |
| Tab callback input | Rust tab facts: stable ID, title, cwd, domain, host, user, process, remote and activity metadata |
| Window callback input | `profile`, `private`, `selected_space`, `space`, `active_tab`, `settings` |
| `title` | Display title string |
| `routing` | Existing space ID or `nil` |
| `filter` | Boolean visibility |
| `theme` | Window callback; validated theme-color overrides |
| `footer` | Window callback; newline string, up to 16 string rows, or `nil` |
| Scheduling | Semantic metadata changes; cached results, no frame-time callbacks |
| Stale results | Discarded after superseding host/model/configuration changes |
| Failure | Valid state retained; two-second batch deadline; warnings deduplicated per configuration epoch |

**Native API**

| Name | Value |
| --- | --- |
| `wezterm.native_tabs.capability` | Current integration contract marker |
| `wezterm.native_tabs.schema` | Rust settings schema |
| `wezterm.native_tabs.configure(options)` | Validated configuration update |
| `wezterm.native_tabs.dispatch(window, action)` | Semantic action on one GUI window |
| `wezterm.native_tabs.inspect(window)` | Async diagnostic projection, geometry, model summary and native CPU timing counters |

Use [development.md](development.md) for installation, build/update commands and GUI verification. Storage boundaries are documented in [protocol.md](protocol.md).
