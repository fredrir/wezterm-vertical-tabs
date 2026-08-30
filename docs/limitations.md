## Notes and limitations

- The sidebar is a real pane in each tab. Pane-directional navigation should
  use `vtabs.action.activate_pane_direction` so the sidebar is skipped.
  `TogglePaneZoom` hides it until unzoomed.
- A tab gets its sidebar when it is first activated, so opening a window with
  many tabs costs one pane split, not one per tab. Tabs spawned by bindings
  other than the plugin's are activated by WezTerm, so they still get theirs at
  once; `vtabs.action.new_tab` splits explicitly.
- The sidebar process runs in the tab's domain; WezTerm cannot host a local
  pane inside a remote-mux tab. On remote multiplexer domains (ssh/tls mux) the
  plugin runs an inline bootstrap that downloads a release build for the remote
  architecture; until a release exists, build `wez-vtabs` on the remote host and
  point `backend.path[domain]` at it. A domain whose backend never answers gets
  one warning and no further sidebars there for 60 s.
- Only pins and closed-tab history persist, to
  `$XDG_STATE_HOME/wez-vtabs/state.json` (default `~/.local/state`, mode 0600,
  written tmp + rename, symlinks refused). Pane/tab/window ids and tokens never
  reach disk. Pins are keyed by tab id, so they are dropped at startup unless a
  sidebar pane survived in the mux; collapsed, focus and private are per GUI
  process.
- WezTerm's own `SplitPane` acts on whichever pane is active, and under
  `hover = "follow"` that is the sidebar whenever the pointer is over it, which
  leaves a shell in a column too narrow to use. `vtabs.action.split(dir)` splits
  the tab's content pane instead. A split that lands in the sidebar's own
  columns anyway is moved to the content side on the next poll, using
  `wezterm cli split-pane --move-pane-id`; where that CLI is unusable (the GUI
  is not on its own socket) the plugin warns once and leaves the pane alone.
- "Move to new window" (drag to the inner edge, menu, or `tear_off`) only works
  for tabs with a single content pane; multi-pane tabs show a notification.
- The action menu is composited into the sidebar pane's own cells. A plugin has
  no surface above the content pane, so the menu can never be drawn over the
  editor; it opens at the column that was clicked and slides back inside the
  sidebar's own columns rather than overlapping anything. Widening the pane
  while the menu is open would move the editor rather than cover it, at one
  reflow each way.
- Tab-close confirmation is the plugin's own popover level, not WezTerm's
  `CloseCurrentTab { confirm = true }` overlay: that overlay replaces the tab's
  panes and is dismissed by the mouse release that follows the click which asked
  for it, so it is unusable from a sidebar click. WezTerm's overlay is still
  used when no sidebar can draw the question.
- Private windows unset shell history via environment variables; shells whose
  rc files re-set `HISTFILE` will still write history. Closed private tabs are
  not recorded for reopening. This is not an isolation boundary.
- Only panes that echoed a token this process minted may drive the sidebar; user
  vars from other panes are ignored. Titles are stripped of control characters
  before rendering.
- Every sidebar backend, including one on a remote mux host, receives every
  tab's title and cwd in its frames.
- A sidebar whose backend stops answering pings for 20 s is restarted.

## Sidebar identity

| Rank | Evidence | Grants |
| --- | --- | --- |
| 3 | pane echoed a token this process minted for it | frames, events, width, close |
| 2 | plugin split the pane in this process | kept out of `content` |
| 1 | pane title `wez-vtabs:<hex>` | kept out of `content`, 5 `auth` attempts, then content again |
| 0 | anything else | content |

One pane per tab holds the role: highest rank wins, every other pane is content.
A faked title can neither empty a tab nor displace a live sidebar.

Rank 1 is a real grant. Any process that can set its pane title — including one
on an ssh/mux host you have a tab in — is sent `auth` on its own stdin, echoes
it, and is then that tab's sidebar: it receives every frame for the window (every
tab's title and cwd, local tabs included) and its events drive tab management,
including `close_tab` without a prompt. A remote host can only do this in tabs of
its own domain, within 30 s and 5 attempts per pane.

| Config | Adopts in |
| --- | --- |
| `adopt = "auto"` (default) | local/unix domains, domains this process already spawned a backend in, domains listed in `backend.path` |
| `adopt = true` | any domain, still only the tab's own domain |
| `adopt = false` | nowhere; a marker pane is content, a surviving sidebar is replaced instead |

Two GUI processes attached to one mux both manage the same tabs and fight over
the sidebars. Unsupported.

## Key and paste forwarding

| Sent to the content pane | Behaviour |
| --- | --- |
| keys, and pastes into an app with `?2004h` | bracketed, so escapes stay data |
| pastes into an app without `?2004h` | arrives raw, exactly as a clipboard paste would: `send_paste` strips `ESC[201~` and turns `\r` into `\n`, other escapes pass through |

## Remote text in frames

| Source | Reaches |
| --- | --- |
| OSC 7 cwd, including its `user@host` | every sidebar in the window, on the meta line of that tab's card |
| pane and tab titles | every sidebar in the window |

A remote shell controls those strings. They are stripped to valid UTF-8 with no control
characters before rendering, so they cannot inject escapes or crash a frame, but their
*content* — a username, a host, a path — is shown on every sidebar of the window, including
sidebars running on other hosts.

Forwarding only ever reaches the source tab's own content pane, in its own domain.
