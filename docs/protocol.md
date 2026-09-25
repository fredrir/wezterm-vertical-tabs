# Contracts

| Name              | Value                                                     |
| ----------------- | --------------------------------------------------------- |
| GUI integration   | Statically linked Rust provider; compile-time contract    |
| UI transport      | Ratatui cells → upstream Termwiz lines → WezTerm renderer |
| Projection        | Ordered stable tab IDs and selected ID or empty selection |
| Geometry          | One window-owned sidebar/content reservation              |
| Storage transport | One JSON request on stdin; one JSON response on stdout    |
| Storage helper    | `wez-vtabs-store --db PATH`                               |
| Protocol version  | `1`                                                       |
| Mux wire          | Upstream protocol plus optional PDUs 63–72                |

| Mux PDU              | Value                                                                   |
| -------------------- | ----------------------------------------------------------------------- |
| `GetHostInfo`        | `{pane_id?}` → `{os, hostname}`                                         |
| `GetPaneCloseInfo`   | `{pane_id}` → `{prompt, process}`                                       |
| `GetPaneLocation`    | `{pane_id}` → `{cwd, home, repo_root?}`                                 |
| `ListAdoptablePanes` | `{domain}` → `{panes}`; attaches `domain` first                         |
| `AdoptPane`          | `{domain, remote_pane_id, window_id?}` → `{pane_id, tab_id, window_id}` |
| Relay                | Intermediate mux answers for the pane's owner                           |
| Stock server         | Rejects the PDU; client keeps upstream labels                           |

**Storage request**

```json
{
  "version": 1,
  "request_id": 42,
  "private": false,
  "operations": [
    {
      "op": "put",
      "key": {
        "scope": { "kind": "profile", "profile": "default" },
        "entity": "settings",
        "field": "width"
      },
      "value": 300,
      "expected_revision": 0
    }
  ]
}
```

| Name                     | Value                                                                             |
| ------------------------ | --------------------------------------------------------------------------------- |
| `read`                   | `{op, scope}`; returns fields including tombstones                                |
| `put`                    | `{op, key, value, expected_revision}`                                             |
| `delete`                 | `{op, key, expected_revision}`; retains a tombstone revision                      |
| Profile scope            | `{kind: "profile", profile}`                                                      |
| Session scope            | `{kind: "session", profile, incarnation}`; caller must verify incarnation         |
| Key                      | `{scope, entity, field}`                                                          |
| Expected revision `0`    | Field has never existed                                                           |
| Expected revision `null` | Unconditional operation                                                           |
| Other expected revision  | Compare the current field revision                                                |
| Batch                    | One transaction; any error rolls back the whole batch                             |
| `private: true`          | Writes rejected; confidential live state is never serialized as a durable request |

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
        "entity": "settings",
        "field": "width"
      },
      "value": 300,
      "revision": 1
    }
  ]
}
```

| Name                        | Value                                                                               |
| --------------------------- | ----------------------------------------------------------------------------------- |
| `error`                     | Optional `{code, message, key?, actual_revision?}`                                  |
| Codes                       | `invalid_request`, `conflict`, `private_write`, `database`, `newer_schema`, `limit` |
| Exit status                 | `0` success; `1` structured request/database error; `2` stdout failure              |
| Request ID                  | Echoed; malformed input uses `0`                                                    |
| Revision                    | Monotonic committed database revision; field-level revisions drive conflicts        |
| Omitted `value` in a record | Tombstone; an explicit `value: null` remains a stored JSON null                     |

| Limit               | Value                                     |
| ------------------- | ----------------------------------------- |
| Request bytes       | 1 MiB                                     |
| Response bytes      | 4 MiB                                     |
| Operations          | 128                                       |
| Returned records    | 4096                                      |
| Value bytes         | 64 KiB                                    |
| Key component bytes | 256                                       |
| SQLite busy timeout | 1500 ms                                   |
| GUI helper deadline | 3 seconds; child killed/reaped on timeout |

| Name                 | Value                                                                        |
| -------------------- | ---------------------------------------------------------------------------- |
| Settings and spaces  | Lua `settings_file`, not SQLite; see [configuration](configuration.md)       |
| Profile fields       | `folder:*`, `catalog.folder_order`, `catalog.derived`, `space:*.collapsed`   |
| Session fields       | `tab:*.membership`, `window:*.selected_space`; verified session identity     |
| Legacy fields        | `settings`, `catalog.order`, `catalog.templates`, other `space:*` ignored    |
| Write coalescing     | 100 ms; memory stays authoritative for rendering/navigation                  |
| Other windows        | Notified after commit; independent GUI clients refresh on focus              |
| Private windows      | Live-tab state and reopen history excluded; shared catalog edits kept        |
