## Notes and limitations

- The sidebar is a real pane in each tab. Pane-directional navigation should
  use `vtabs.action.activate_pane_direction` so the sidebar is skipped.
  `TogglePaneZoom` hides it until unzoomed.
- Tabs spawned by bindings other than the plugin's get their sidebar on the
  next poll (`poll_ms`). Use `vtabs.action.new_tab` for an instant one.
- The sidebar process runs in the tab's domain. For remote multiplexer domains
  (ssh/tls mux) the bootstrap script and `cargo` fallback are not available on
  the remote side; set `domain = "local"` to run the sidebar locally, or install
  `wez-vtabs` remotely and point `backend.path` at it.
- Pins, sidebar identities and closed-tab history persist to
  `$XDG_STATE_HOME/wez-vtabs/state.json` (default `~/.local/state`), so they
  survive restarts against a persistent mux server. Collapsed/focus state is
  per GUI process.
- "Move to new window" (drag to the inner edge, menu, or `tear_off`) only works
  for tabs with a single content pane; multi-pane tabs show a notification.
- Private windows unset shell history via environment variables; shells whose
  rc files re-set `HISTFILE` will still write history. Closed private tabs are
  not recorded for reopening. This is not an isolation boundary.
- Only registered sidebar panes may drive the sidebar; user vars from other
  panes are ignored. Titles are stripped of control characters before rendering.
- A sidebar whose backend stops answering pings for 20 s is restarted.
