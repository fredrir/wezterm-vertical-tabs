-- Generated from vtabs-core; edit the Rust types.
---@meta

---@alias TabsShortcut 'settings'|'tabs'|'commands'|'tab_actions'|'space_actions'|'quick_terminal'|'rail'|'new_tab'|'close'|'refresh'|'tab_1'|'tab_2'|'tab_3'|'tab_4'|'tab_5'|'tab_6'|'tab_7'|'tab_8'|'tab_9'|'next_tab'|'previous_tab'|'rename_tab'|'pin_tab'|'move_tab_top'|'move_tab_bottom'|'move_tab_space'|'move_tab_folder'|'move_tab_window'|'close_tabs_below'|'close_other_tabs'|'return_tab_auto'|'create_space'|'rename_space'|'edit_space_icon'|'edit_space_accent'|'edit_space_rules'|'move_space_up'|'move_space_down'|'delete_space'|'toggle_space'|'space_1'|'space_2'|'space_3'|'space_4'|'space_5'|'space_6'|'space_7'|'space_8'|'space_9'|'new_folder'|'new_tab_in_folder'|'toggle_folder'|'rename_folder'|'move_folder_up'|'move_folder_down'|'delete_folder'|'reset_settings'|'previous_space'|'next_space'

---@class TabsSettings
---@field keybinds? table<TabsShortcut, string[]> Shortcuts supplied by this plugin
---@field width? integer Width in logical pixels
---@field side? 'left'|'right' Sidebar edge
---@field rail? 'expanded'|'hidden' Sidebar visibility
---@field quick_terminal_width? number Fraction of the window width
---@field quick_terminal_height? number Fraction of the window height
---@field quick_terminal_window_padding? TabsWindowPadding Sides like WezTerm window_padding; unset sides follow it
---@field quick_terminal_blur? integer Backdrop blur radius in logical pixels behind a translucent background
---@field quick_terminal_opacity? number Background opacity; 1 is opaque and text stays opaque
---@field quick_terminal_border_color? string Null follows the terminal foreground at 22% opacity; none hides it
---@field quick_terminal_background_color? string Null follows the quick terminal palette background
---@field quick_terminal_auto_hide? boolean Hide the quick terminal when the window loses focus
---@field quick_terminal_preserve_shell? boolean Keep the shell when hidden; disable to start a fresh shell each time
---@field quick_terminal_hide_on_escape? 'never'|'always'|'auto' Auto hides only while nothing runs in the shell
---@field animations? boolean Enable finite visual transitions
---@field reduced_motion? boolean Suppress transitions
---@field animation_ms? integer Transition duration in milliseconds
---@field cards? boolean Use cards instead of compact rows
---@field show_indexes? boolean Visible tab indices
---@field show_metadata? boolean Show the session domain
---@field show_close? boolean Show close controls
---@field tab_glyphs? boolean Show machine glyphs on tabs
---@field glyph_size? number Sidebar glyph size relative to the text
---@field tab_title? 'directory'|'process'|'pane_title'|'process_and_directory' Title source; renaming a tab overrides it
---@field confirm_close? boolean Confirm closing a tab with a running process
---@field accent? string Default accent color
---@field background? string Surface background
---@field foreground? string Text foreground
---@field muted? string Secondary text foreground
---@field selected_background? string Selected card background
---@field hover_background? string Hovered card background; null follows Background
---@field tab_foreground? string Tab title text; null follows Foreground
---@field selected_foreground? string Selected and hovered card text; null follows Accent
---@field local_glyph_color? string Local machine glyph; null follows the tab text
---@field distro_colors? table<'arch'|'ubuntu'|'debian'|'fedora'|'nixos'|'alpine'|'centos'|'rhel'|'opensuse'|'manjaro'|'raspbian'|'gentoo'|'freebsd'|'linux'|'macos'|'windows'|'remote', string> Remote glyph colors by os-release ID, `remote` when unknown
---@field keyboard_shortcuts? boolean Enable tab, search, settings and sidebar shortcuts
---@field default_domain? string Spawn domain used in an empty space; null retains the host default
---@field menus? TabsMenuEntry[] Nested semantic action menus

---@class TabsTheme
---@field accent? string Default accent color
---@field background? string Surface background
---@field foreground? string Text foreground
---@field muted? string Secondary text foreground
---@field selected_background? string Selected card background
---@field hover_background? string Hovered card background; null follows Background
---@field tab_foreground? string Tab title text; null follows Foreground
---@field selected_foreground? string Selected and hovered card text; null follows Accent
---@field local_glyph_color? string Local machine glyph; null follows the tab text
---@field distro_colors? table<'arch'|'ubuntu'|'debian'|'fedora'|'nixos'|'alpine'|'centos'|'rhel'|'opensuse'|'manjaro'|'raspbian'|'gentoo'|'freebsd'|'linux'|'macos'|'windows'|'remote', string> Remote glyph colors by os-release ID, `remote` when unknown

--- Argument to `vtabs.action` and `wezterm.vtabs.dispatch`.
---@alias TabsAction TabsUiAction|TabsIntent

--- Pixels, or a string with a `px`, `%`, `pt` or `cell` unit, read like WezTerm's `window_padding`.
---@alias TabsDimension number|string

---@alias TabsHideOnEscape 'never'|'always'|'auto'

---@alias TabsIntent 'ActivateLast'|'NewTab'|'ResetSettings'|{ SelectSpace: string }|{ CreateSpace: { name: string } }|{ RenameSpace: { id: string, name: string } }|{ EditSpace: { accent?: string, icon: string, id: string, rules: TabsRoutingRule[] } }|{ DeleteSpace: { destination?: string, id: string } }|{ MoveSpace: { id: string, index: integer } }|{ ToggleSpace: string }|{ CreateFolder: { name: string } }|{ RenameFolder: { id: string, name: string } }|{ ToggleFolder: string }|{ DeleteFolder: string }|{ AssignFolder: { folder_id?: string, tab_id: integer } }|{ MoveFolder: { id: string, index: integer } }|{ ActivateTab: integer }|{ ActivateIndex: integer }|{ ActivateRelative: { delta: integer, wrap: boolean } }|{ NewTabInFolder: string }|{ CloseTab: integer }|{ CloseOthers: integer }|{ CloseBelow: integer }|{ RenameTab: { id: integer, title: string } }|{ PinTab: { id: integer, pinned: boolean } }|{ MoveTab: { id: integer, index: integer } }|{ AssignTab: { id: integer, space_id: string } }|{ ReturnToAuto: integer }|{ SetSetting: { key: TabsSettingKey, value: any } }|{ ResetSetting: TabsSettingKey }|{ SetRail: TabsRailMode }|{ MoveTabToNewWindow: integer }|{ CustomAction: string }

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

--- A fresh program in the quick terminal surface; ends when hidden unless `preserve` is set.
---@class TabsQuickProgram
---@field args? string[] The domain's default program when empty.
---@field auto_hide? boolean `quick_terminal_auto_hide` when omitted.
---@field blur? integer Backdrop blur radius in logical pixels, at most 64; `quick_terminal_blur` when omitted.
---@field border_color? string A WezTerm color, or `none`; `quick_terminal_border_color` when omitted.
---@field colors? table WezTerm `colors` over the terminal palette; `background` also fills the frame.
---@field cwd? string The active pane's directory when omitted.
---@field domain? TabsSpawnDomain The active pane's domain when omitted.
---@field height? number Fraction of the window height, above 0 and at most 1; `quick_terminal_height` when omitted.
---@field hide_on_escape? TabsHideOnEscape `quick_terminal_hide_on_escape` when omitted.
---@field preserve? boolean Keep the program running while hidden; it ends with the window.
---@field set_environment_variables? table<string, string>
---@field width? number Fraction of the window width, above 0 and at most 1; `quick_terminal_width` when omitted.
---@field window_background_opacity? number From 0 through 1; `quick_terminal_opacity` when omitted.
---@field window_padding? TabsWindowPadding Sides left out follow `quick_terminal_window_padding`.

---@alias TabsRailMode 'expanded'|'hidden'

---@class TabsRoutingRule
---@field fields? { [1]: TabsMatchField, [2]: string[] }[]
---@field remote? boolean

---@alias TabsSettingKey 'keybinds'|'width'|'side'|'rail'|'quick_terminal_width'|'quick_terminal_height'|'quick_terminal_window_padding'|'quick_terminal_blur'|'quick_terminal_opacity'|'quick_terminal_border_color'|'quick_terminal_background_color'|'quick_terminal_auto_hide'|'quick_terminal_preserve_shell'|'quick_terminal_hide_on_escape'|'animations'|'reduced_motion'|'animation_ms'|'cards'|'show_indexes'|'show_metadata'|'show_close'|'tab_glyphs'|'glyph_size'|'tab_title'|'confirm_close'|'accent'|'background'|'foreground'|'muted'|'selected_background'|'hover_background'|'tab_foreground'|'selected_foreground'|'local_glyph_color'|'distro_colors'|'keyboard_shortcuts'|'default_domain'|'menus'

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

--- WezTerm's `SpawnTabDomain`.
---@alias TabsSpawnDomain 'DefaultDomain'|'CurrentPaneDomain'|{ DomainName: string }|{ DomainId: integer }

--- Host metadata and application-owned state, also used by the transfer contract.
---@class TabsTab
---@field bell? boolean
---@field cwd? string
---@field domain? string
---@field folder_id? string
---@field home? string The owning machine's home; this host's when omitted.
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
---@field home? string The owning machine's home; this host's when omitted.
---@field id integer
---@field left? integer Cell extent inside the tab, so the sidebar can mirror the split layout.
---@field os? string
---@field remote? boolean Splits of one tab can live on different machines.
---@field repo_root? string
---@field title? string
---@field top? integer
---@field width? integer

--- Sidebar surfaces opened by `vtabs.action`, besides domain intents.
---@alias TabsUiAction 'settings'|'create_space'|'navigator'|'tab_actions'|'space_actions'|'quick_terminal'|'retry_storage'|{ QuickTerminal: TabsQuickProgram }

--- Input to the window-level `theme` and `footer` hooks.
---@class TabsWindowContext
---@field active_tab? integer
---@field profile string
---@field selected_space string
---@field settings TabsSettings
---@field space TabsSpace

--- Sides left out follow WezTerm's `window_padding`; a quick program's follow the quick terminal's.
---@class TabsWindowPadding
---@field bottom? TabsDimension
---@field left? TabsDimension
---@field right? TabsDimension
---@field top? TabsDimension

---@class TabsHooks
---@field title? fun(tab: TabsTab): string? Display title
---@field routing? fun(tab: TabsTab): string? Existing space ID
---@field filter? fun(tab: TabsTab): boolean? Visibility
---@field theme? fun(context: TabsWindowContext): TabsTheme? Theme-color overrides
---@field footer? fun(context: TabsWindowContext): string|string[]|nil Rows shown above spaces
---@field adoptable? fun(): string|string[]|nil Domains whose unadopted panes the tab search lists, besides connected ones

---@class TabsOptions
---@field profile? string Shared catalog scope; default `default`
---@field settings? TabsSettings Owned by this config; read-only in the settings UI
---@field spaces? TabsSpace[] Owned by this config; listed before spaces created in the UI
---@field templates? TabsSpaceTemplate[] Dynamic spaces derived from tab metadata
---@field hooks? TabsHooks
---@field settings_file? string File the settings UI writes; default `wezterm.config_dir .. "/vtabs_settings.lua"`

--- Module returned by `plugin/init.lua`.
---@class TabsPlugin
---@field apply_to_config fun(config: table, options?: TabsOptions): table
---@field action fun(action: TabsAction): { EmitEvent: string }
