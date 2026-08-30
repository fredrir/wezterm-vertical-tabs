## Notes and limitations

- The sidebar is a real pane in each tab (WezTerm has no custom-chrome API).
  Pane-directional navigation should use `vtabs.action.activate_pane_direction`
  so the sidebar is skipped.
- Tabs spawned by bindings other than the plugin's get their sidebar on the
  next poll (`poll_ms`). Use `vtabs.action.new_tab` for an instant one.
- For remote multiplexer domains (ssh/tls mux) the sidebar process runs on the
  remote side, so `wez-vtabs` must be installable there (cargo or release).
- Pin state lives in `wezterm.GLOBAL` and survives config reloads but not a
  full restart.
- "Move to new window" moves the tab's panes; split layout is rebuilt
  approximately for multi-pane tabs.