# Behavior-tree benchmark kit

A shared workload model, host, form registry and measurement library for native
and WebAssembly comparisons. This crate is unpublished. It is a default workspace
member so normal CI tests every registered form and the comparison protocol.

The host is a counting `CallBridge` implemented in Rust. It exercises the actual
behavior-tree executor and its indirect dispatches; it does not load guest modules
through an engine. A module call is a call of one of this host's functions. A
dispatch is an `arora_call_indirect`, including control nodes and the root.

## Measurement and agreement

Times are microseconds per tick, including clock updates and the host's input
writes. Loading, lowering, digesting and summarizing are outside the timed
interval. The host writes only changed values; for keys also written by the tree
it compares with the store. Each sample replays a freshly lowered tree from its
initial state, including warm-up. Samples interleave forms. Each logical tick's
time is its median over samples; the summary reports the mean, p50, p95, p99,
maximum and phase means of those times after warm-up. Quantiles interpolate
linearly between neighboring order statistics. Counts are means over measured
ticks from the first replay. The raw JSON retains every replay's per-tick times,
module calls and dispatches, including warm-up.

After each tick of the first replay, a digest covers every sink key and every key
the tree can write, in key-index order. Scale keys follow graph-call order.
Digests include absent values, boolean tags and little-endian f64 bits, with signed
zero and NaNs canonicalized, using FNV-1a. `differing` counts ticks whose digest
differs from `detached`, including warm-up. The reference is evaluated even if a
selection omits it. Front ends fail on disagreement. Hashes detect accidental
changes; they are not cryptographic proofs of equivalence.

The registered forms are:

- `attached-naive`: every graph call is a leaf of a root `PARALLEL` and runs
  every tick. Calls transitively reading a key the tree writes follow the tree;
  other calls precede it, in data order. A reader uses a sink's store key or an
  unpublished call's tree-local return binding.
- `detached`: graph calls are detached nodes referenced by node return ports.
  Sinks run after the tree; a branch's `publish` leaves demand its graph calls.
  The executor caches unchanged inputs and evaluates a demanded node at most once
  per tick. This is the agreement reference.

## Workload format

This README defines the JSON contract. Labels and other descriptive properties
are optional and ignored. Numbers become f64, booleans remain booleans, and null
initial values mean absent keys. All indices are zero-based.

| field | meaning |
|---|---|
| `keys` | `[{"name": "input/x", "initial": 0.0}, ...]`; unique, nonempty names |
| `calls` | graph calls in data order, each `{"op": "sum", "args": [["a0", source], ...], "publishes": key_or_null}` |
| `tree` | a tree node from the vocabulary below |
| `ticks`, `warmUp` | total replay ticks and the initial ticks excluded from timing summaries; `0 <= warmUp < ticks` |
| `clock`, `dt` | a key index and positive seconds per tick; before tick t, the host writes `(t + 1) * dt` |
| `events` | `[[tick, key, scalar], ...]`, applied in listed order within a tick, after its clock write |
| `phases` | `{"names": ["idle", "active"], "runs": [[0, 0], [80, 1]]}`; sorted starts, phase 0 before the first start |

A source is exactly one of `{"key": k}`, `{"node": c}` or `{"value": scalar}`.
A graph source may reference only an earlier graph call. Argument and output role
strings are descriptive: position determines the parameter's UUID.

Tree nodes:

| `k` | other fields and behavior |
|---|---|
| `parallel`, `seq`, `fallback` | `children: [node, ...]`; nonempty native composites |
| `equal` | `key`, `value`: native equality check |
| `call` | `op`, `args: [[role, source], ...]`, `outputs: [[role, key], ...]`, `state: n`; a host action returning Status |
| `publish` | `node`, `key`: calls the host's identity function with its return bound to the key |
| `calls` | `calls: [c, ...]`: placement markers for forms that use explicit placement; the two current forms ignore them |
| `run` | native RUN |

Graph operations, with arguments in order:

| operation | result |
|---|---|
| `sum`, `product`, `max` | reduction of one or more arguments |
| `sub(x,y)`, `div(x,y)` | subtraction, division; division by zero returns zero |
| `clamp(x,lo,hi)` | `max(x,lo).min(hi)` |
| `remap(x,inLo,inHi,outLo,outHi)` | affine remap; a zero input range returns outLo |
| `overlay(x,unused,unused,base)` | `base + x` |
| `first(x)`, `publish(x)` | x |
| `mix(x...)` | sum of `x[i] * 0.5^(i+1)` |
| `noise(x,y)` | `sin(1.3*x + 0.7*y)` |
| `osc(frequency,phase,time)` | `sin(2*pi*frequency*time + phase)` |
| `damp(target,halfLife)` | starts at target; updates state by `state + (target-state)*(1-exp2(-(1/30)/halfLife))`, snapping within 1e-4 |
| `chain-add(x,y)`, `chain-mul(x,y)` | x+y, x*y; signature also has unbound mutable state and output |
| `chain-clamp(x,y)`, `chain-remap(x,y)` | `max(x,0).min(abs(y)+1)`, `(x-y)*0.5`, with the same chain signature |

Damping state is a mutable literal initialized to NaN, private to its call. The
fixed 1/30-second damping time base is independent of the workload clock step.

Tree actions have private mutable states initialized to NaN, and change output
keys only when the action writes a different value. Timed actions restart when
their last-call state is NaN or more than 0.025 seconds before t; the executor has
no halt callback.

| action | outputs; state parameters; result |
|---|---|
| `above(x,threshold)` | no outputs or state; Success if x>threshold, else Failure |
| `hold(t,duration,v...)` | one output per v; since,last; writes v on restart, Running until positive duration elapses, then Success; duration 0 remains Running |
| `follow(v...)` | one output per v, no state; writes v, Running |
| `pulse(t,gapMin,gapMax,low,high,width,salt)` | one output; last,next,release; after random gaps writes a level in [low,high], then zero after width, Running |
| `wander(t,gapMin,gapMax,salt,(low,high)...)` | one output per range; last,next; writes a new level per range after random gaps, Running |
| `keyframes(t,n,(offset,v1..vn)...)` | n outputs; since,last,frame; writes frames at offsets after restart, Success after the last frame, otherwise Running |
| `set(v...)` | one output per v, no state; writes v, Success |

Random choices are deterministic functions of time and salt. The host rounds t to
microseconds, XORs it with the hashed rounded salt, then hashes using xor-shifts
16,15,16 and wrapping u32 multipliers 0x7feb352d and 0x846ca68b, dividing by 2^32.
Pulse draws gaps at salt and salt+2, and levels at salt+1. Wander draws gaps at
salt on restart and salt+100 thereafter, and levels at salt+1+output-index.
Offsets and durations use a 1e-9 tolerance. See [host.rs](src/host.rs) for the
arithmetic and [model.rs](src/model.rs) for validation.

The built-in synthetic workload is generated by [synthetic.rs](src/synthetic.rs).
It has 18 graph calls, two graph sinks, damping, and four host-event phases with
follow, pulse/wander, hold/keyframes and set branches. It contains no device data.

## Scale scenarios

Default sizes are 10, 30, 100, 300 and 1000 graph calls. Every scenario uses the
same model, host, form construction and measurement code as JSON workloads.

- `settled`, `one-input`, `all-inputs`: K=max(2,N/10) independent chains of add,
  multiply, clamp and remap, with 1.1 as their second argument. The last call of
  each chain is a sink. Inputs remain constant, input 0 changes, or every input
  changes to `tick % 17`.
- `clock`: a deterministic random graph; one tenth of its calls are oscillators
  reading time. Other calls are sum, product, clamp, remap or damping.
- `branches`, `branches-switching`: eight graphs of ceil(N/8) calls guarded by
  native equality checks under a fallback. Every graph input changes every tick;
  one branch remains active or the active branch changes every 50 ticks. Only
  the active branch publishes.

The random generator is a u32 LCG with seed 332396, multiplier 1664525 and
increment 1013904223. Each graph has max(2,n/10) inputs. Calls read an input or a
previous call within 32 positions. Unread calls publish, with at least ceil(n/10)
published calls. Generation is deterministic; branch sizes can round above N.
Every scenario includes the host clock update in its timing.

The switching scenario replaces damping with identity while preserving its graph
edges. Continuously running inactive damping and pausing it on inactive branches
have different reentry semantics. Stateless switching isolates branch scheduling
and permits tick-for-tick agreement. Fixed branches and the clock graph retain
damping; the synthetic workload also exercises stateful tree actions and sinks.

## Adding a form

1. Add a module under [src/forms](src/forms) implementing `Form`.
2. Implement `build(&Workload) -> Nodes`. Shared attached/detached construction is
   available in `forms::build`; the model and schema nodes are public.
3. Override `lower` when needed. It receives nodes, names, the variable resolver,
   function index and mutable bridge. A placement pass can run before loading;
   a form can choose lowering options or set flags on the lowered tree before
   returning it. These choices stay in the form's module.
4. Declare `fixture_counts`: total (module calls, dispatches), warm-up included,
   for the synthetic fixture, then the six scenarios in `Scenario::ALL` order,
   each at N=10 with 16 warm-up and 144 measured ticks. Update only the new form's
   expectations, based on its execution semantics.
5. Declare the module and add its instance to `FORMS`. Every harness, name list,
   agreement check and fixture test uses that registry. Keep detached registered
   as the reference.

The incremental, placed, detached-full and inferred forms belong to their
respective executor changes; they are not registered here. No behavior-tree
runtime changes are needed for the two current forms.

## Native use

From the workspace root, with its pinned nightly and normal platform tools:

```sh
cargo test --release -p arora-behavior-tree-bench
cargo bench -p arora-behavior-tree-bench --bench workload -- synthetic 7
cargo bench -p arora-behavior-tree-bench --bench workload -- /path/workload.json 7 raw.json
cargo bench -p arora-behavior-tree-bench --bench scale -- 500 7
cargo bench -p arora-behavior-tree-bench --bench scale -- 500 7 1000/clock/detached
```

Workload arguments: path (or `synthetic`, the default), positive odd sample count
(default 7), optional raw JSON path. `WORKLOAD_ONLY=detached,attached-naive`
selects forms, and `WORKLOAD_TIMES=raw.json` supplies a JSON path when no third
argument is given. Unknown or duplicate form names fail.

Scale arguments: measured ticks (500), positive odd samples (7), optional exact
`N/scenario/form` cell. `SCALE_SIZES=10,100`, `SCALE_FORMS=detached,attached-naive`,
`SCALE_WARM_UP=200` and `SCALE_TIMES=scale.json` configure its matrix and raw output.

For one process per complete round:

```sh
cargo build --release -p arora-behavior-tree-bench --bin bench-kit
BENCH_SAMPLES=1 target/release/bench-kit round.json /path/workload.json
```

This always includes synthetic and all scale scenarios. Optional workloads use
their file stems as test names, so stems must be distinct and different from
`synthetic`. Configuration: `BENCH_FORMS`, `BENCH_SIZES`, `BENCH_TICKS` (500),
`BENCH_WARM_UP` (200), `BENCH_SAMPLES` (1).

## Node and browser use

The benchmark cdylib targets **wasm32-wasip1**, WASI preview 1. This gives the same
Rust `Instant` clock and stderr interface to both hosts. It is separate from the
repository's wasm32-unknown-unknown browser engine and wasm32-wasip2 guest
components. The loader supplies clock, randomness, stderr and the small remaining
WASI interface; it requires no npm packages.

```sh
rustup target add wasm32-wasip1
cargo build --release -p arora-behavior-tree-bench --target wasm32-wasip1 --lib
cargo run --release -p arora-behavior-tree-bench --bin synthetic-workload > target/bench-synthetic.json
node crates/arora-behavior-tree-bench/js/run-node.mjs \
  target/wasm32-wasip1/release/arora_behavior_tree_bench.wasm target/bench-synthetic.json
node crates/arora-behavior-tree-bench/js/check.mjs \
  target/wasm32-wasip1/release/arora_behavior_tree_bench.wasm target/bench-synthetic.json
```

Node 18+ is supported. The Node runner accepts additional workload paths, prints
the native table columns and fails on disagreement. The same `BENCH_*` variables
configure Node and the native round binary. `NODE_TIMES=wasm.json` saves raw data.
`NODE_NATIVE=round.json` additionally checks exact per-tick call and dispatch
counts and identical test sets against a native round. Use the same workload
stems in both files; for example, save synthetic as `synthetic.json` for a check
against the native round's built-in `synthetic` test. Do not compare native and
WASI value digests.

For a browser, build the module and generate `target/bench-synthetic.json` as
above, then serve the workspace root:

```sh
python3 -m http.server 8000
```

Open `http://localhost:8000/crates/arora-behavior-tree-bench/js/browser.html`.
The page runs the synthetic workload and every scenario at N=100, yields between
samples, and can save raw results. A file picker accepts another workload without
copying it into the repository. The loader is an ES module usable by other pages:

```js
import { loadBenchWasm } from './bench-wasm.mjs';
const bench = await loadBenchWasm(bytes);
const result = await bench.loadWorkload(jsonText).measureAll({ samples: 7 });
const scale = await bench.loadScale(100, 'clock').measureAll({ samples: 7 });
```

Only one workload/scale session is live per module instance. Loading another
replaces the previous session. Calls are synchronous; use a Worker for an
interactive page during long replays.

The loader measures the smallest positive `performance.now()` step across 32
clock transitions, then requires each sample's measured tick intervals to sum to
at least 50 steps. Short samples repeat the entire workload from a fresh tree;
per-tick times are means across these repetitions. The outer sample median is
unchanged. `clock.stepUs`, `clock.minimumSampleUs` and `crossOriginIsolated` are
recorded with results, along with repetition counts and individual replay times.
Per-tick quantiles remain limited by clock resolution; averaging repeated ticks
improves a short sample's total estimate but does not recover sub-step latency
outliers. Calibration costs at most 32 steps in usual clocks; a clock that never
advances is rejected.

### C ABI

All export names start with `bench_`. Pointers address linear memory. Names and
result buffers are borrowed until the next state-changing export; copy before
calling another export. Row access outside its range returns NaN. Status exports
return 0 on success and 1 for invalid input; an executor failure traps with stderr.

| exports | contract |
|---|---|
| `alloc(len)`, `free(ptr,len)` | initialized input allocation, freed exactly once |
| `names()`, `names_len()` | UTF-8 `scenario,...;form,...`, registry order |
| `measure(n,scenario,form,warmUp,ticks,samples)` | one scale cell, indices into names |
| `row(i)` | last scale row: 0–2 sample-mean median/q1/q3 µs; 3–4 mean calls/dispatches; 5–6 high/low u32 of aggregate per-tick digest |
| `workload_load(ptr,len)` | parse workload UTF-8 JSON; replaces current session |
| `scale_load(n,scenario,warmUp,ticks)` | initialize a scale session for interleaved sampling |
| `workload_names()`, `workload_names_len()` | `form,...;phase,...;graphCalls,ticks,warmUp,dt` |
| `workload_begin(form)` | reset this form's runs and select it |
| `workload_select(form)` | select a form without clearing runs, for interleaving |
| `workload_sample()` | one logical sample; returns its positive sample count, or 0 if not begun |
| `workload_finish()` | summarize selected form; requires positive odd samples; consumes its runs |
| `workload_row(i)` | 0–4 mean/p50/p95/p99/max µs; 5–6 calls/dispatches; 7 differing ticks; 8–9 high/low aggregate digest; 10+ phase means |
| `workload_series()`, `workload_series_len()` | f64 triples per 15 ticks after warm-up: time, calls, phase; length counts f64 elements |
| `result()`, `result_len()` | last form's full raw JSON record, identical schema to native |
| `minimum_sample_us(us)` | nonnegative timed budget per sample; 0 disables repetition |

The cdylib uses the workspace release profile (optimized, thin LTO, debug level
1). Native and wasm builds use that same profile; comparison runs should record
any profile overrides and toolchain changes alongside their results.

## Comparison protocol

```sh
ROUNDS=7 BENCH_FORMS=detached,attached-naive \
  crates/arora-behavior-tree-bench/scripts/run.sh /tmp/bench-results \
  base=<revision> candidate=<revision> another=<revision> -- /path/workload.json
python3 crates/arora-behavior-tree-bench/scripts/analyze.py /tmp/bench-results \
  --baseline base --forms detached,attached-naive
```

Revisions must contain this crate and the chosen forms. The Bash runner creates
one detached worktree and target directory per revision, builds the round binary
with `--release --offline --locked`, and runs one process per build per round.
Order is forward on odd rounds and reversed on even rounds. It leaves worktrees,
resolved revision IDs, raw rounds and comparison output under the requested output
directory. Use a new directory for each run. Dependencies must be available locally;
platform compiler settings may be provided through the invoking environment.

The Python analysis uses the standard library only. It rejects missing or extra
rounds, missing tests/forms, different replay dimensions, cross-platform input,
nonfinite times and any digest disagreement across forms, builds or rounds.
Each replay contributes the mean time after warm-up, and a round's value is the
mean of its replay means. A workload ratio is the build's mean over rounds divided
by the baseline build's mean over rounds, for the same form. A scale scenario's
ratio is the geometric mean of those ratios over its sizes. Overall is the
geometric mean over test types: each workload and each scenario receives one
weight, regardless of graph size or duration. Separate geometric means cover
workloads and scenarios.

Intervals are the 2.5th and 97.5th percentiles of 2,000 bootstrap estimates, with
seed 20261025. Each build's rounds are drawn independently with replacement;
the same draw is used across that build's tests and sizes to preserve the
covariance of a process-level round. Every ratio and geometric mean is recomputed
for every draw. Results are written as `compare.md` and `compare.json`. A ratio
below 1 means less time; intervals describe observed session variability, not
portability to another machine.

Follow these rules:

- **Check agreement before reading times.** A faster form that writes different
  values is not faster.
- **Always time a control form the change does not touch.** Giving an inlined
  function a second caller can prevent inlining and move an untouched form by
  several percent. Individual tests can move about ±2% between builds from code
  layout alone.
- **One process per build per round, alternating order.** Thermal drift and
  background load then fall on both builds alike.
- **Compare call and dispatch counts across machines, times only within one.**
  Counts describe executor work; elapsed times depend on hardware and load.
- **Compare digests only within one platform.** Native and WebAssembly take
  `sin` and `exp2` from different math libraries and can differ in their last bits.
- **The browser clock is coarsened.** Cross-origin isolation usually improves
  precision; retain the measured step, isolation flag and repetition counts when
  interpreting results.

## Checks

```sh
cargo test --release -p arora-behavior-tree-bench
cargo clippy -p arora-behavior-tree-bench --all-targets -- -D warnings
cargo fmt -p arora-behavior-tree-bench --check
cargo bench -p arora-behavior-tree-bench --bench workload --bench scale --no-run
python3 -B crates/arora-behavior-tree-bench/scripts/test_analyze.py
```

The registry test runs synthetic plus N=10 of every scenario, compares every tick
with detached and asserts each form's total module calls and dispatches. The
Python checks run under `cargo test` as well. The existing CI package selection
includes this default member and its tests and lint targets. WASI/Node execution
is a separate local check; there is no additional WASI CI job.
