-- Generated from vtabs-core; edit the Rust types.
---@meta

---@class TabsSettings
---@field width? integer Expanded width in logical pixels
---@field rail_width? integer Collapsed width in logical pixels
---@field side? 'left'|'right' Sidebar edge
---@field rail? 'expanded'|'collapsed'|'hidden' Sidebar visibility
---@field animations? boolean Enable finite visual transitions
---@field reduced_motion? boolean Suppress transitions
---@field animation_ms? integer Transition duration in milliseconds
---@field cards? boolean Use cards instead of compact rows
---@field show_indexes? boolean Visible tab indices
---@field show_metadata? boolean Show the session domain
---@field show_close? boolean Show close controls
---@field confirm_close? boolean Confirm closing a tab with a running process
---@field accent? string Default accent color
---@field background? string Surface background
---@field foreground? string Text foreground
---@field muted? string Secondary text foreground
---@field selected_background? string Selected card background
---@field private_accent? string Private window accent
---@field distro_colors? table<string, string> Remote glyph colors by os-release ID, `remote` when unknown
---@field keyboard_shortcuts? boolean Enable tab, search, settings and sidebar shortcuts
---@field reopen_limit? integer Maximum in-memory launch intents
---@field default_domain? string Spawn domain used in an empty space; null retains the host default
---@field private_env? table<string, string> Environment additions for explicitly created private windows
---@field menus? TabsMenuEntry[] Nested semantic action menus

---@class TabsTheme
---@field accent? string Default accent color
---@field background? string Surface background
---@field foreground? string Text foreground
---@field muted? string Secondary text foreground
---@field selected_background? string Selected card background
---@field private_accent? string Private window accent
---@field distro_colors? table<string, string> Remote glyph colors by os-release ID, `remote` when unknown

--- Argument to `vtabs.action` and `wezterm.vtabs.dispatch`.
---@alias TabsAction TabsUiAction|TabsIntent

---@alias TabsIntent 'ActivateLast'|'NewTab'|'Reopen'|'ResetSettings'|'PrivateWindow'|{ SelectSpace: string }|{ CreateSpace: { name: string } }|{ RenameSpace: { id: string, name: string } }|{ EditSpace: { accent?: string, icon: string, id: string, rules: TabsRoutingRule[] } }|{ DeleteSpace: { destination?: string, id: string } }|{ MoveSpace: { id: string, index: integer } }|{ ToggleSpace: string }|{ CreateFolder: { name: string } }|{ RenameFolder: { id: string, name: string } }|{ ToggleFolder: string }|{ DeleteFolder: string }|{ AssignFolder: { folder_id?: string, tab_id: integer } }|{ MoveFolder: { id: string, index: integer } }|{ ActivateTab: integer }|{ ActivateIndex: integer }|{ ActivateRelative: { delta: integer, wrap: boolean } }|{ NewTabInFolder: string }|{ CloseTab: integer }|{ CloseOthers: integer }|{ RenameTab: { id: integer, title: string } }|{ PinTab: { id: integer, pinned: boolean } }|{ MoveTab: { id: integer, index: integer } }|{ AssignTab: { id: integer, space_id: string } }|{ ReturnToAuto: integer }|{ SetSetting: { key: TabsSettingKey, value: any } }|{ ResetSetting: TabsSettingKey }|{ SetRail: TabsRailMode }|{ MoveTabToNewWindow: integer }|{ CustomAction: string }

---@class TabsLaunchSpec
---@field args? string[]
---@field cwd? string
---@field domain? string
---@field env? table<string, string>

--- The plugin-owned Lua file the settings UI rewrites; hand edits are reloaded by WezTerm.
---@class TabsManaged
---@field settings? TabsSettings
---@field spaces? TabsSpace[]
---@field templates? TabsSpaceTemplate[]

---@alias TabsMatchField 'domain'|'host'|'user'|'process'|'cwd'|'title'

---@class TabsMenuEntry
---@field action? string
---@field children? TabsMenuEntry[]
---@field confirm? boolean
---@field id string
---@field label string

---@alias TabsRailMode 'expanded'|'collapsed'|'hidden'

---@class TabsRoutingRule
---@field fields? { [1]: TabsMatchField, [2]: string[] }[]
---@field remote? boolean

---@alias TabsSettingKey 'width'|'rail_width'|'side'|'rail'|'animations'|'reduced_motion'|'animation_ms'|'cards'|'show_indexes'|'show_metadata'|'show_close'|'confirm_close'|'accent'|'background'|'foreground'|'muted'|'selected_background'|'private_accent'|'distro_colors'|'keyboard_shortcuts'|'reopen_limit'|'default_domain'|'private_env'|'menus'

---@class TabsSpace
---@field accent? string
---@field collapsed? boolean Hides this space's pinned tabs and folders; indexes stay stable.
---@field icon? string
---@field id string
---@field name string
---@field rules? TabsRoutingRule[]
---@field template? string

---@class TabsSpaceTemplate
---@field accent? string
---@field icon? string
---@field id string
---@field name string
---@field rules TabsRoutingRule[]

--- Host metadata is copied only when it changes; membership remains application-owned.
---@class TabsTab
---@field bell? boolean
---@field cwd? string
---@field domain? string
---@field folder_id? string
---@field home? string The owning machine's home; `None` falls back to this host's.
---@field host? string
---@field icon? string
---@field id integer
---@field launch? TabsLaunchSpec
---@field manual_assignment? boolean
---@field os? string os-release style ID of the machine running the tab's active pane.
---@field panes? TabsTabPane[]
---@field pinned? boolean
---@field process? string
---@field remote? boolean
---@field repo_root? string Repository root when the directory is inside a Git work tree.
---@field space_id? string
---@field title string
---@field title_hook? string
---@field title_override? string
---@field unread? boolean
---@field user? string

---@class TabsTabPane
---@field active? boolean
---@field cwd? string
---@field height? integer
---@field home? string The owning machine's home; `None` falls back to this host's.
---@field id integer
---@field left? integer Cell extent inside the tab, so the sidebar can mirror the split layout.
---@field os? string
---@field remote? boolean Splits of one tab can live on different machines.
---@field repo_root? string
---@field title? string
---@field top? integer
---@field width? integer

--- Sidebar surfaces opened by `vtabs.action`, besides domain intents.
---@alias TabsUiAction 'settings'|'create_space'|'navigator'|'jobs'|'retry_storage'

--- Input to the window-level `theme` and `footer` hooks.
---@class TabsWindowContext
---@field active_tab? integer
---@field private boolean
---@field profile string
---@field selected_space string
---@field settings TabsSettings
---@field space TabsSpace

---@class TabsHooks
---@field title? fun(tab: TabsTab): string? Display title
---@field routing? fun(tab: TabsTab): string? Existing space ID
---@field filter? fun(tab: TabsTab): boolean? Visibility
---@field theme? fun(context: TabsWindowContext): TabsTheme? Theme-color overrides
---@field footer? fun(context: TabsWindowContext): string|string[]|nil Rows shown above spaces

---@class TabsOptions
---@field profile? string Shared catalog scope; default `default`
---@field settings? TabsSettings Owned by this config; read-only in the settings UI
---@field spaces? TabsSpace[] Owned by this config; listed before spaces created in the UI
---@field templates? TabsSpaceTemplate[] Dynamic spaces derived from tab metadata
---@field hooks? TabsHooks
---@field settings_file? string File the settings UI writes; default `wezterm.config_dir .. "/vtabs_settings.lua"`
