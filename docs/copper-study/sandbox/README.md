# Copper sandbox — runnable evidence for the study

A standalone crate (detached from the Arora workspace via an empty
`[workspace]` table) that pulls in the **real** Copper runtime and a real
behavior-tree library, so the study's claims are checked by running code, not
by reading docs.

## Versions exercised

| Crate | Version | Role |
|-------|---------|------|
| `cu29` (+ `cu29-runtime`, `cu29-derive`) | `1.0.0-rc2` | Copper runtime + `#[copper_runtime]` macro |
| `wasmtime` | `45.0.1` | dynamic-dispatch cost model (Arora's wasm path) |
| `bonsai-bt` | `0.12.1` | off-the-shelf Rust behavior tree |
| `bincode` | `2` | serialization-cost proxy |
| toolchain | `nightly 1.98 (2026-06-02)` | — |

> The earlier `studio-bridge/copper-study` was written against Copper **0.15**.
> Copper has since moved to a **1.0 release candidate** and the task API changed
> (see "API deltas" below). All numbers here are from `1.0.0-rc2`.

## Run it

```sh
cd docs/copper-study/sandbox
cargo run  --release --bin minimal_pipeline   # static DAG compiled from RON, runs
cargo run  --release --bin throughput         # ns per runtime iteration
cargo run  --release --bin wasm_dispatch      # dynamic vs static call cost
cargo run  --release --bin bt_bonsai          # a real behavior tree + trees-as-data
cargo run  --release --bin shared_data        # zero-copy shared data across the boundary
cargo test --release                          # asserts the DAG runs from RON
```

## What each file proves

| File | Claim it grounds |
|------|------------------|
| `src/minimal_pipeline.rs` + `copperconfig.ron` | A Copper app **is** a compile-time DAG: the `#[copper_runtime(config=...)]` macro reads the RON at build time and generates the runtime. |
| `build.rs` | Copper apps **require** a `build.rs` exporting `LOG_INDEX_DIR` — the macro panics without it. |
| `src/throughput.rs` | One full 3-task iteration costs **~410 ns** (incl. per-iteration unified logging); message passing itself is a typed memory write, no serde. |
| `src/wasm_dispatch.rs` | A dynamic (wasm) module call costs **~100+ ns** vs **~1.3 ns** for a static Rust call — ~80-90x. Quantifies the "dynamic-loading overhead" hypothesis. |
| `src/bt_bonsai.rs` | A real BT ticks correctly and **serialises to JSON and back** (trees-as-data), but its leaf action set is a **compile-time enum**. |
| `src/shared_data.rs` | Zero-copy shared store slice across the module boundary: native pointer (~free), one wasm linear memory shared across instances (verified), memcpy vs serialise (~5x). |
| `tests/pipeline_test.rs` | `cargo test` green: the static DAG compiles from RON and produces the expected output. |

## Captured results (release, macOS arm64)

```text
minimal_pipeline : RESULT last_collected=10
throughput       : RESULT copper_iter_ns=409.8 iters=200000 total_ms=82.0
wasm_dispatch    : RESULT direct_rust_ns=1.26 wasm_guest_call_ns=20.91 \
                          wasm_host_roundtrip_ns=58.55 bincode_roundtrip_ns=27.71
                   MODEL  arora_dynamic_call_min_ns ~= roundtrip + 2*bincode = 114.0
bt_bonsai        : [battery-ok]   battery=80 last_action=Wave     final_position=150
                   [battery-low]  battery=5  last_action=GoCharge final_position=-1
                   [reloaded-from-json] ... final_position=150
                   RESULT json_len=183 pos_full=150 pos_low=-1 pos_reloaded=150 roundtrip_ok=true
shared_data      : RESULT native_zero_copy_ns=3.71 cross_module_shared_ok=true \
                          wasm_memcpy64_ns=7.67 bincode64_ns=40.93
cargo test       : test copper_static_dag_runs_from_ron ... ok
```

## API deltas vs the 0.15 study (found by compiling)

The `1.0.0-rc2` task traits differ from what the `studio-bridge` study described:

- Tasks must implement `Freezable + Reflect` (was `Freezable` only); derive
  `Reflect` from the prelude.
- `fn new(config, resources: Self::Resources<'_>)` — a new `Resources` associated
  type and a `resources` argument.
- `fn process(&mut self, ctx: &CuContext, ...)` — `&CuContext`, not `&RobotClock`.
- Build/run: `App::builder().with_log_path(path, Some(slab))?.build()?`, then
  `start_all_tasks` / `run_one_iteration` / `stop_all_tasks`.
