# Native contracts

| Name | Value |
| --- | --- |
| GUI integration | Statically linked Rust provider; compile-time contract |
| UI transport | Ratatui cells → upstream Termwiz lines → native WezTerm renderer |
| Native projection | Ordered stable tab IDs and selected ID or empty selection |
| Geometry | One window-owned sidebar/content reservation |
| Storage transport | One JSON request on stdin; one JSON response on stdout |
| Storage helper | `wez-vtabs-store --db PATH` |
| Protocol version | `1` |
| Mux wire | Unchanged upstream protocol |

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

| Name | Value |
| --- | --- |
| `read` | `{op, scope}`; returns fields including tombstones |
| `put` | `{op, key, value, expected_revision}` |
| `delete` | `{op, key, expected_revision}`; retains a tombstone revision |
| Profile scope | `{kind: "profile", profile}` |
| Session scope | `{kind: "session", profile, incarnation}`; caller must verify incarnation |
| Key | `{scope, entity, field}` |
| Expected revision `0` | Field has never existed |
| Expected revision `null` | Unconditional operation |
| Other expected revision | Compare the current field revision |
| Batch | One transaction; any error rolls back the whole batch |
| `private: true` | Writes rejected; confidential live state is never serialized as a durable request |

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

| Name | Value |
| --- | --- |
| `error` | Optional `{code, message, key?, actual_revision?}` |
| Codes | `invalid_request`, `conflict`, `private_write`, `database`, `newer_schema`, `limit` |
| Exit status | `0` success; `1` structured request/database error; `2` stdout failure |
| Request ID | Echoed; malformed input uses `0` |
| Revision | Monotonic committed database revision; field-level revisions drive conflicts |
| Omitted `value` in a record | Tombstone; an explicit `value: null` remains a stored JSON null |

| Limit | Value |
| --- | ---: |
| Request bytes | 1 MiB |
| Response bytes | 4 MiB |
| Operations | 128 |
| Returned records | 4096 |
| Value bytes | 64 KiB |
| Key component bytes | 256 |
| SQLite busy timeout | 1500 ms |
| GUI helper deadline | 3 seconds; child killed/reaped on timeout |

The application coalesces field changes and keeps memory authoritative for rendering/navigation. Successful writes notify other windows in the same GUI; independent GUI clients refresh on focus. Profiles/settings survive sessions. Live assignments/pins restore only with verified session identity. Private live-tab state and reopen history are excluded; explicit shared catalog/settings edits remain durable.
