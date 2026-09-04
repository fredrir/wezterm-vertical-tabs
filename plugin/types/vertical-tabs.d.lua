---@meta
-- luacheck: ignore 212

---Public API returned by `wezterm.plugin.require` for wezterm-vertical-tabs.
---@class VerticalTabs
---@field version string Plugin version.
---@field action VerticalTabs.ActionTable Actions suitable for WezTerm key bindings.
local M = {}

---@class (exact) VerticalTabs.Settings
---@field persist? boolean Persist settings-page changes that differ from the defaults.
---@field path? string Absolute path to the settings file.

---@class (exact) VerticalTabs.Frame
---@field zen? boolean Enable the zen frame.
---@field radius? integer Card corner radius in device pixels.
---@field margin? integer Outer frame margin in device pixels.
---@field inset? integer Air inside the card, clamped to `margin`.
---@field border? boolean|string `true` derives a border, `false` hides it, and `"accent"` uses the active accent; another string is used as a colour.
---@field border_width? number Border stroke width in device pixels.

---@class (exact) VerticalTabs.CustomStripAction
---@field id string Stable id for this button.
---@field icon? string Glyph drawn in the top strip.
---@field on_click fun(window: Window) Called when the button is clicked.

---@alias VerticalTabs.StripAction
---|"toggle"
---|"new_tab"
---|"settings"
---|"search"
---|VerticalTabs.CustomStripAction

---@class (exact) VerticalTabs.KeyBinding
---@field key string WezTerm key name.
---@field mods? string WezTerm modifier expression, such as `"CTRL|SHIFT"`.

---Overrides for the plugin's named key bindings. Set an entry to `false` to disable it.
---@class (exact) VerticalTabs.KeyMap
---@field toggle_sidebar? false|VerticalTabs.KeyBinding
---@field toggle? false|VerticalTabs.KeyBinding Alias for `toggle_sidebar`.
---@field focus_sidebar? false|VerticalTabs.KeyBinding
---@field new_tab? false|VerticalTabs.KeyBinding
---@field close_tab? false|VerticalTabs.KeyBinding
---@field reopen_closed? false|VerticalTabs.KeyBinding
---@field pin_tab? false|VerticalTabs.KeyBinding
---@field private_window? false|VerticalTabs.KeyBinding
---@field private_window_alt? false|VerticalTabs.KeyBinding
---@field new_window? false|VerticalTabs.KeyBinding
---@field next_tab? false|VerticalTabs.KeyBinding
---@field prev_tab? false|VerticalTabs.KeyBinding
---@field next_tab_alt? false|VerticalTabs.KeyBinding
---@field prev_tab_alt? false|VerticalTabs.KeyBinding
---@field next_tab_arrow? false|VerticalTabs.KeyBinding
---@field prev_tab_arrow? false|VerticalTabs.KeyBinding
---@field move_tab_up? false|VerticalTabs.KeyBinding
---@field move_tab_down? false|VerticalTabs.KeyBinding
---@field tab_1? false|VerticalTabs.KeyBinding
---@field tab_2? false|VerticalTabs.KeyBinding
---@field tab_3? false|VerticalTabs.KeyBinding
---@field tab_4? false|VerticalTabs.KeyBinding
---@field tab_5? false|VerticalTabs.KeyBinding
---@field tab_6? false|VerticalTabs.KeyBinding
---@field tab_7? false|VerticalTabs.KeyBinding
---@field tab_8? false|VerticalTabs.KeyBinding
---@field tab_last? false|VerticalTabs.KeyBinding
---@field settings? false|VerticalTabs.KeyBinding
---@field open_settings? false|VerticalTabs.KeyBinding
---@field next_space? false|VerticalTabs.KeyBinding
---@field prev_space? false|VerticalTabs.KeyBinding
---@field tear_off? false|VerticalTabs.KeyBinding
---@field rename_tab? false|VerticalTabs.KeyBinding

---@class (exact) VerticalTabs.Rgb
---@field [1] integer Red channel, from 0 to 255.
---@field [2] integer Green channel, from 0 to 255.
---@field [3] integer Blue channel, from 0 to 255.

---A colour string in config, or an RGB triple copied from a resolved-theme hook argument.
---@alias VerticalTabs.ThemeColor string|VerticalTabs.Rgb

---Partial theme overrides. Unspecified colours are derived from the active WezTerm palette.
---@class (exact) VerticalTabs.Theme
---@field bg? VerticalTabs.ThemeColor
---@field fg? VerticalTabs.ThemeColor
---@field elevation? number Sidebar darkening fraction, from 0 to 0.3.
---@field accent? VerticalTabs.ThemeColor
---@field private_accent? VerticalTabs.ThemeColor
---@field hover_bg? VerticalTabs.ThemeColor
---@field hover_fg? VerticalTabs.ThemeColor
---@field active_bg? VerticalTabs.ThemeColor
---@field active_fg? VerticalTabs.ThemeColor
---@field focus_bg? VerticalTabs.ThemeColor
---@field meta_fg? VerticalTabs.ThemeColor
---@field dim? VerticalTabs.ThemeColor
---@field title_idle? VerticalTabs.ThemeColor
---@field title_active? VerticalTabs.ThemeColor
---@field active_title_fg? VerticalTabs.ThemeColor Legacy alias for `title_active`.
---@field pinned_fg? VerticalTabs.ThemeColor
---@field separator? VerticalTabs.ThemeColor
---@field border? VerticalTabs.ThemeColor
---@field border_idle? VerticalTabs.ThemeColor
---@field ghost_border_hover? VerticalTabs.ThemeColor
---@field new_tab_fg? VerticalTabs.ThemeColor
---@field close_fg? VerticalTabs.ThemeColor
---@field close_hover_fg? VerticalTabs.ThemeColor
---@field unseen_fg? VerticalTabs.ThemeColor
---@field drag_bg? VerticalTabs.ThemeColor
---@field drag_fg? VerticalTabs.ThemeColor
---@field scroll_fg? VerticalTabs.ThemeColor
---@field scroll_idle_fg? VerticalTabs.ThemeColor
---@field surface_raised? VerticalTabs.ThemeColor
---@field scrim? number Popover background fade fraction.
---@field disabled_fg? VerticalTabs.ThemeColor
---@field popover_sel_bg? VerticalTabs.ThemeColor
---@field popover_sel_fg? VerticalTabs.ThemeColor
---@field popover_sel_hint? VerticalTabs.ThemeColor
---@field split? string `"auto"` keeps WezTerm's divider, `"hidden"` hides it, and another value is used as its colour.

---Complete theme passed to `hooks.theme`; colours are RGB triples.
---@class (exact) VerticalTabs.ResolvedTheme
---@field bg VerticalTabs.Rgb
---@field fg VerticalTabs.Rgb
---@field dim VerticalTabs.Rgb
---@field accent VerticalTabs.Rgb
---@field title_idle VerticalTabs.Rgb
---@field meta_fg VerticalTabs.Rgb
---@field active_bg VerticalTabs.Rgb
---@field active_fg VerticalTabs.Rgb
---@field hover_bg VerticalTabs.Rgb
---@field hover_fg VerticalTabs.Rgb
---@field focus_bg VerticalTabs.Rgb
---@field pinned_fg VerticalTabs.Rgb
---@field separator VerticalTabs.Rgb
---@field border VerticalTabs.Rgb
---@field border_idle VerticalTabs.Rgb
---@field ghost_border_hover VerticalTabs.Rgb
---@field new_tab_fg VerticalTabs.Rgb
---@field close_fg VerticalTabs.Rgb
---@field close_hover_fg VerticalTabs.Rgb
---@field unseen_fg VerticalTabs.Rgb
---@field private_accent VerticalTabs.Rgb
---@field drag_bg VerticalTabs.Rgb
---@field drag_fg VerticalTabs.Rgb
---@field scroll_fg VerticalTabs.Rgb
---@field scroll_idle_fg VerticalTabs.Rgb
---@field title_active VerticalTabs.Rgb
---@field active_title_fg VerticalTabs.Rgb
---@field title_active_contrast number
---@field content_bg VerticalTabs.Rgb
---@field surface_raised VerticalTabs.Rgb
---@field scrim number
---@field disabled_fg VerticalTabs.Rgb
---@field popover_sel_bg VerticalTabs.Rgb
---@field popover_sel_fg VerticalTabs.Rgb
---@field popover_sel_hint VerticalTabs.Rgb

---@alias VerticalTabs.Glob string|string[]

---@class (exact) VerticalTabs.SpaceMatch
---@field domain? VerticalTabs.Glob
---@field host? VerticalTabs.Glob
---@field user? VerticalTabs.Glob
---@field proc? VerticalTabs.Glob
---@field title? VerticalTabs.Glob
---@field cwd? VerticalTabs.Glob A path without `*` also matches its descendants.
---@field remote? boolean

---@class (exact) VerticalTabs.Space
---@field id string Space id; `$domain`, `$host`, `$user`, `$proc`, and `$cwd` create templates.
---@field name? string Display name; defaults to `id`.
---@field icon? string One glyph for the space switcher.
---@field theme? "auto"|VerticalTabs.Theme Per-space overrides, `"auto"`, or nil to inherit.
---@field match? VerticalTabs.SpaceMatch Routing rule; every supplied field must match.

---@class (exact) VerticalTabs.RouteMeta
---@field tab_id integer
---@field window_id integer
---@field title string
---@field proc? string
---@field cwd? string
---@field host? string
---@field user? string
---@field domain? string
---@field remote boolean
---@field space? string
---@field manual boolean

---@class (exact) VerticalTabs.FooterEntry
---@field id? string Stable id for the row.
---@field text? string Footer text.
---@field icon? string Leading glyph.
---@field fg? VerticalTabs.Rgb
---@field bg? VerticalTabs.Rgb
---@field icon_fg? VerticalTabs.Rgb
---@field on_click? fun(window: Window, entry: VerticalTabs.FooterEntry)

---@alias VerticalTabs.TitleCallback fun(tab: MuxTab, pane: Pane): string?
---@alias VerticalTabs.FilterCallback fun(tab: MuxTab, mux_window: MuxWindow): boolean
---@alias VerticalTabs.FooterCallback fun(mux_window: MuxWindow): (string|VerticalTabs.FooterEntry)[]?
---@alias VerticalTabs.ThemeCallback fun(window: Window, theme: VerticalTabs.ResolvedTheme): VerticalTabs.Theme?
---@alias VerticalTabs.RouteCallback fun(meta: VerticalTabs.RouteMeta): string?
---@alias VerticalTabs.BackendPathCallback fun(domain: string?, host: string?): string?
---@alias VerticalTabs.BackendPath string|table<string, string>|VerticalTabs.BackendPathCallback

---@class (exact) VerticalTabs.Backend
---@field path? VerticalTabs.BackendPath Path to `wez-vtabs`, optionally selected by domain or OSC 7 host.
---@field repo? string GitHub repository used for release downloads.
---@field version? string Release tag to download.
---@field build? boolean Fall back to `cargo build` when no release matches.
---@field uservar? string WezTerm user-variable name used by the backend.
---@field env? table<string, string> Extra environment; plugin-owned `VTABS_*` values win.
---@field inbox? boolean Offer the same-machine inbox transport.

---@alias VerticalTabs.SplitDirection "Right"|"Left"|"Top"|"Bottom"|"Up"|"Down"
---@alias VerticalTabs.PaneDirection "Down"|"Left"|"Next"|"Prev"|"Right"|"Up"
---@alias VerticalTabs.SplitCommand SpawnCommand|fun(pane: Pane, window: Window): SpawnCommand?

---@class (exact) VerticalTabs.ActionTable
---@field toggle_sidebar Action
---@field focus_sidebar Action
---@field new_tab Action
---@field close_tab Action
---@field reopen_closed Action
---@field pin_tab Action
---@field private_window Action
---@field new_window Action
---@field tear_off Action
---@field rename_tab Action
---@field next_tab Action
---@field prev_tab Action
---@field move_tab_up Action
---@field move_tab_down Action
---@field next_space Action
---@field prev_space Action
---@field open_settings Action
---@field activate_tab fun(index: integer): Action Activates a visible tab by zero-based index; `-1` selects the last.
---@field activate_pane_direction fun(direction: VerticalTabs.PaneDirection): Action Skips the sidebar pane.
---@field split fun(direction: VerticalTabs.SplitDirection, spawn?: VerticalTabs.SplitCommand): Action Splits the content pane, never the sidebar.
---@field switch_space fun(id: string): Action
---@field move_to_space fun(id: string): Action

---Apply the plugin to a WezTerm configuration.
---@param config Config
---@param opts? VerticalTabs.Config
---@return Config config
function M.apply_to_config(config, opts) end

---Toggle the sidebar in a GUI window.
---@param window Window
function M.toggle_sidebar(window) end

---Explicitly show or hide the sidebar in a GUI window.
---@param window Window
---@param shown boolean
function M.show_sidebar(window, shown) end

---Publish the current state for a GUI window.
---@param window Window
function M.sync(window) end

---Clear cached resolved theme state for one window, or every window when omitted.
---@param window_id? integer
function M.invalidate_theme(window_id) end

---Whether a pane presents as a vertical-tabs backend. This is suitable for skipping, not trust.
---@param pane Pane
---@return boolean
function M.is_sidebar_pane(pane) end

---Supply the content pane's title while the sidebar is the active pane.
---@param tab TabInformation
---@param pane PaneInformation
---@param tabs TabInformation[]
---@param panes PaneInformation[]
---@return string?
function M.window_title(tab, pane, tabs, panes) end

---Whether the GUI window is a private vertical-tabs window.
---@param window Window
---@return boolean
function M.is_private_window(window) end

M.version = ""
M.action = {} --[[@as VerticalTabs.ActionTable]]

return M
