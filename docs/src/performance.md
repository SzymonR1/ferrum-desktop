# Performance and accessibility evidence

**Status:** Experimental, single-host evidence. These samples are not guarantees,
benchmarks across platforms, or proof of full screen-reader support.

## Reference host and dataset

Measurements were collected on 2026-10-09 on Fedora Linux 44 with KDE Plasma,
using the X11 backend. The host has an Intel Core i7-14700KF (20 physical cores,
28 logical CPUs) and 31 GiB of RAM. Docker Engine reported version 29.8.2 and API
1.56. The live application sample read the current local engine inventory of 14
containers and did not mutate engine resources.

## Observed samples

| Workload | Dataset and method | Observed result |
|---|---|---|
| Release process startup to inventory | Two release-mode Ferrum processes launched on X11; elapsed monotonic time from spawn to the `Loaded local container inventory` log. The filesystem cache was warm; these are not cold-start measurements. | 101 ms and 167 ms; 14 containers per run |
| Connected-process memory | `/proc/<pid>/status` RSS sampled after inventory load and after a 30-second idle interval in each run. | Run 1: 107.3 MiB at load and after 30 seconds; Run 2: 107.7 MiB at load, 110.6 MiB after 30 seconds |
| Connected-process CPU | `/proc/<pid>/stat` user/system tick deltas over the same 30-second intervals; `getconf CLK_TCK` reported 100 ticks/second. | Run 1: 0.00 s; Run 2: 0.11 s of process CPU time |
| Inventory filter and sort | Release-mode `InventoryView` benchmark; 1,000 synthetic summaries, 20 warmups and 200 rebuilds alternating an empty query with `container-09`. Each row has an ID, name, one of eight image names, Running state, and `Up` status. | 253 μs median; 408 μs p95 per rebuild |
| Log-buffer append | Release-mode `LogBuffer` benchmark; 100,000 synthetic 512-byte stdout lines per run, one warmup and seven measured runs. The buffer retains at most 10,000 lines or 8 MiB. | 1,967 μs median; 3,002 μs maximum; 5,120,000 retained bytes |

The connected-process RSS sample exceeds the initial 100 MiB GUI-only target and
is not a measurement of GUI-only memory. No claim is made that the target is met.

## Resilience and bounded data

Headless tests cover loading, disconnected inventory, refresh failure while
preserving the last successful inventory, inspection loading, log-stream
failure/reconnect, terminal-open failure, and input disablement at stream EOF.
The current in-memory limits are 10,000 retained log lines or 8 MiB, a
16-message log channel, a terminal output channel of 64 chunks × 4 KiB, at most
16 terminal output chunks drained per UI frame, 64 queued terminal input
messages with a 64 KiB per-message limit, and 10,000 VT scrollback rows.

## Accessibility evidence and limits

Headless `egui_kittest` tests validate AccessKit roles and labels, the labelled
Search field, AccessKit activation, keyboard Tab/Space row activation, terminal
focus and Ctrl+C, and log loading/error/reconnect controls. The 1,000-row test
confirms fewer than 100 row buttons are materialized in its viewport and that
the final row remains searchable. This exercises the accessibility tree and
keyboard path, not native screen-reader speech, visual focus treatment, or GPU
scroll-frame time; no high-throughput live-engine stream measurement was made.
The desktop provider cannot capture screenshots or enumerate Ferrum, so native
screen-reader and visual validation remain untested; see the [platform support
matrix](platform-support.md).

## Reproducing the samples

Run from the repository root. The release UI sample is opt-in, requires an active
X11 display and an accessible local engine, uses an isolated temporary Ferrum
settings directory, reads inventory only, and terminates only its own app
process. The inventory and log-buffer samples use deterministic synthetic data
and do not connect to an engine.

```sh
cargo test -p ferrum-app --release --test performance --locked -- --ignored --nocapture
cargo test -p ferrum-app --release --locked measures_filter_and_sort_for_one_thousand_container_records -- --ignored --nocapture
cargo test -p ferrum-app --release --locked measures_bounded_log_buffer_append_throughput -- --ignored --nocapture
cargo test -p ferrum-app --locked large_inventory_is_virtualized_and_accessible_to_keyboard_users
```

The benchmark tests have no timing thresholds and are excluded from the default
suite. Results depend on host load, filesystem cache, display server, engine
inventory, and compiler/toolchain; compare only runs with those details recorded.
No GPU scroll latency, cross-platform performance, or full-container-engine
1,000-record dataset was measured.