# Performance

```sh
just test bench
just test bench -- core
just test bench -- ui
just test bench -- store
cargo xtask check
```

| Work                    | Optimization                                                                      |
| ----------------------- | --------------------------------------------------------------------------------- |
| Unchanged snapshots     | Reuse tab metadata and ordering; skip topology allocations                        |
| Metadata updates        | Update changed host fields; rebuild visible ordering only when membership changes |
| Tab labels              | Local probes cached per directory; remote owners probe once per directory change  |
| Sidebar groups          | Group tabs and count folder members once per model revision                       |
| Sidebar painting        | Reuse cached rows, tab numbers and folder counts                                  |
| Surface movement        | Publish transforms using retained cells and hit regions                           |
| Cell changes            | Consume Ratatui's diff iterator without an intermediate vector                    |
| Disabled Lua hooks      | Skip hook metadata copies and queued hook work                                    |
| SQLite reads            | Deferred snapshot transactions; current-schema opens avoid migration write locks  |
| SQLite writes           | Reuse prepared statements; check field revisions only for conditional writes      |
| Storage response limits | Count serialized bytes without temporary JSON buffers                             |
| Helper output           | Buffer JSON writes and flush before exit                                          |

| Measurement | Boundary                                                                                             |
| ----------- | ---------------------------------------------------------------------------------------------------- |
| Build       | Release profile; locked dependencies                                                                 |
| Harness     | Divan; arguments after `--` are Divan options and name filters such as `core`, `ui` or `store`       |
| Timing      | Fastest, slowest, median and mean per operation; fixture preparation excluded                        |
| Samples     | Divan defaults; override with `--sample-count` and `--sample-size`                                   |
| Comparison  | Same harness, dependency lock and machine; stop other builds during measurement                      |
| Allocations | Divan `AllocProfiler`: Rust allocator calls and bytes; excludes SQLite's C allocator and process RSS |
| Rendering   | Headless integration verifies behavior; benchmark timings exclude GPU and physical display latency   |
| Durability  | SQLite schema, journal policy, atomic writes and revision conflicts remain unchanged                 |

Integration commands: [Validation](validation.md).
