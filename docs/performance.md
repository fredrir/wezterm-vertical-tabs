# Performance

```sh
just bench
just bench core
just bench ui
just bench store
just check
```

| Work                    | Optimization                                                                      |
| ----------------------- | --------------------------------------------------------------------------------- |
| Unchanged snapshots     | Reuse tab metadata and ordering; skip topology allocations                        |
| Metadata updates        | Update changed host fields; rebuild visible ordering only when membership changes |
| Sidebar groups          | Group tabs and count folder members once per model revision                       |
| Sidebar painting        | Reuse cached rows, tab numbers and folder counts                                  |
| Surface movement        | Publish transforms using retained cells and hit regions                           |
| Cell changes            | Consume Ratatui's diff iterator without an intermediate vector                    |
| Disabled Lua hooks      | Skip hook metadata copies and queued hook work                                    |
| SQLite reads            | Deferred snapshot transactions; current-schema opens avoid migration write locks  |
| SQLite writes           | Reuse prepared statements; check field revisions only for conditional writes      |
| Storage response limits | Count serialized bytes without temporary JSON buffers                             |
| Helper output           | Buffer JSON writes and flush before exit                                          |

| Measurement | Boundary                                                                                           |
| ----------- | -------------------------------------------------------------------------------------------------- |
| Build       | Release profile; locked dependencies                                                               |
| Timing      | Median nanoseconds per operation; fixture preparation excluded; allocation counting disabled       |
| Samples     | `VTABS_BENCH_SAMPLES=9`, `VTABS_BENCH_ITERATIONS=40`; 10 warmup operations                         |
| Output      | CSV: `case,median_ns,median_allocations,median_requested_bytes`                                    |
| Comparison  | Same harness, dependency lock and machine; stop other builds during measurement                    |
| Allocations | Rust allocator calls and requested bytes; excludes SQLite's C allocator and process RSS            |
| Rendering   | Headless integration verifies behavior; benchmark timings exclude GPU and physical display latency |
| Durability  | SQLite schema, journal policy, atomic writes and revision conflicts remain unchanged               |

Integration commands: [Validation](validation.md).
