# Contracts

| Name | Value |
| --- | --- |
| GUI integration | Statically linked Rust provider; compile-time contract |
| UI transport | Ratatui cells → upstream Termwiz lines → WezTerm renderer |
| Projection | Ordered stable tab IDs and selected ID or empty selection |
| Geometry | One window-owned sidebar/content reservation |
| Storage transport | One JSON request on stdin; one JSON response on stdout |
| Storage helper | `wez-vtabs-store --db PATH` |
| Protocol version | `1` |
| Mux wire | Upstream protocol plus optional PDUs 65-66, 69-82 |

| Mux PDU | ID | Value |
| --- | --- | --- |
| `GetPaneCloseInfo` | 65, 66 | `{pane_id}` → `{prompt, process}` |
| `ListAdoptablePanes` | 69, 70 | `{domain}` → `{panes}`; attaches `domain` first |
| `AdoptPane` | 71, 72 | `{domain, remote_pane_id, window_id?}` → `{pane_id, tab_id, window_id}` |
| `ReplacePane` | 73 | `{pane_id, command?, command_dir?, domain, wait_for_ready}` → `SpawnResponse` |
| `GetPaneLocation` | 74, 75 | `{pane_id?}` → `{cwd, home, repo_root?, os, hostname}`; no `pane_id`: the server |
| `SpawnBesidePane` | 76 | `{pane_id, command?, command_dir?, size, workspace}` → `SpawnResponse`; hidden tab in the pane's domain and directory |
| `FindAdoptablePanes` | 77, 78 | `{domains}` → `{panes: [{domain, entry, location?}]}`; once per server, attaching `domains` first |
| `SendKeyDownIfBusy` | 79, 80 | `{pane_id, event}` → `{sent}`; sends the key unless closing would not prompt |
| `GetMuxSession` | 81, 82 | `{}` → `{session}`; stable for the running mux process across GUI clients and connection routes |
| Retired | 63, 64 | `GetHostInfo`; never reused |
| Retired | 67, 68 | `GetPaneLocation` without host fields; never reused |
| Relay |  | Intermediate mux answers for the pane's owner |
| Stock or older server |  | Rejects unsupported PDUs; tab placement restoration requires `GetMuxSession` |

**Storage request**

```json
{
  "version": 1,
  "request_id": 42,
  "operations": [
    {
      "op": "put",
      "key": {
        "scope": { "kind": "profile", "profile": "default" },
        "entity": "folder:tools",
        "field": "name"
      },
      "value": "Tools",
      "expected_revision": 0
    }
  ]
}
```

| Name | Value |
| --- | --- |
| `read` | `{op, scope}`; returns fields including tombstones |
| `read_key` | `{op, key}`; returns one field including its revision/tombstone, or no record if absent |
| `put` | `{op, key, value, expected_revision}` |
| `delete` | `{op, key, expected_revision}`; retains a tombstone revision |
| Profile scope | `{kind: "profile", profile}` |
| Mux session scope | `{kind: "mux_session", profile, session}` |
| Key | `{scope, entity, field}` |
| Expected revision `0` | Field has never existed |
| Expected revision `null` | Unconditional operation |
| Other expected revision | Compare the current field revision |
| Batch | One transaction; any error rolls back the whole batch |

**Storage response**

```json
{
  "version": 1,
  "request_id": 42,
  "revision": 1,
  "records": [
    {
      "key": {
        "scope": { "kind": "profile", "profile": "default" },
        "entity": "folder:tools",
        "field": "name"
      },
      "value": "Tools",
      "revision": 1
    }
  ]
}
```

| Name | Value |
| --- | --- |
| `error` | Optional `{code, message, key?, actual_revision?}` |
| Codes | `invalid_request`, `conflict`, `database`, `newer_schema`, `limit` |
| Exit status | `0` success; `1` structured request/database error; `2` stdout failure |
| Request ID | Echoed; malformed input uses `0` |
| Revision | Monotonic committed database revision; field-level revisions drive conflicts |
| Omitted `value` in a record | Tombstone; an explicit `value: null` remains a stored JSON null |

| Limit | Value |
| --- | --- |
| Request bytes | 1 MiB |
| Response bytes | 4 MiB |
| Operations | 128 |
| Returned records | 4096 |
| Value bytes | 64 KiB |
| Key component bytes | 256 |
| SQLite busy timeout | 1500 ms |
| GUI helper deadline | 3 seconds; child killed/reaped on timeout |

| Name | Value |
| --- | --- |
| Settings and spaces | Lua `settings_file`, not SQLite; see [configuration](configuration.md) |
| Profile fields | `folder:*`, `catalog.folder_order`, `catalog.derived`, `space:*.collapsed` |
| Session fields | `tab:*` or `pane:*`: `space`, `manual`, `pinned`, `folder`; verified mux session identity |
| Session reads | Exact placement keys, batched within the operation limit; closed-tab history is not loaded |
| Legacy fields | `settings`, `catalog.order`, `catalog.templates`, other `space:*` ignored |
| Write coalescing | 100 ms; memory stays authoritative for rendering/navigation |
| Other windows | Notified after commit; independent GUI clients refresh on focus |