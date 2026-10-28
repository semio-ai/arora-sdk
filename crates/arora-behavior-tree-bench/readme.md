# Behavior-tree benchmark kit

A shared workload model, host, form registry and measurement library for native
and WebAssembly comparisons. This crate is unpublished. It is a default workspace
member so normal CI tests every registered form and the comparison protocol.

The host is a counting `CallBridge` implemented in Rust. It exercises the actual
behavior-tree executor and its indirect dispatches; it does not load guest modules
through an engine. A module call is a host stand-in or a linked real function. A
dispatch is an `arora_call_indirect`, including control nodes and the root.

## Measurement and agreement

Times are microseconds per tick, including clock updates and the host's input
writes. Loading, lowering, digesting and summarizing are outside the timed
interval. The host writes only changed values; for keys also written by the tree
or a graph sink it compares with the store. Each replay starts from a freshly
lowered tree and includes its workload warm-up. A discarded pilot runs real work
before every sample, warming the process's code, allocator and CPU state before
collecting times. Samples interleave forms.

A clock is coarse when its step exceeds one twentieth of the pilot's measured
interval divided by its measured tick count; [clock.rs](src/clock.rs) defines this
rule. Fine clocks time ticks individually: each logical tick's time is its median
over samples, then summaries report the mean, linearly interpolated quantiles,
maximum, phase means and 15-tick bins. Native uses the high resolution `Instant`
clock (step 0 means no coarsening supplied); the WASI loader supplies its calibrated
step. Coarse clocks time all measured ticks of each replay as one interval, with
two clock reads, and divide by the measured tick count. A pilot fixes the number
of replays before collecting a sample, targeting 50 clock steps of total measured
work. The pilot estimate does not guarantee that budget in subsequent replays.
The coarse mean averages interval means across replays and samples. Per-tick
quantiles, maximum, time bins and phase means are unresolved: JSON uses `null`,
tables print `unresolved`, and numeric C rows use NaN.

Raw JSON records each sample's `timingMode` (`tick` or `replay`), `sampleMeans`,
`repetitions`, and clock modes. In tick mode, `times` contains per-tick means over
repetitions and `replayTimes` retains individual repetitions, warm-up included.
In replay mode these per-tick arrays are empty and `replayUs` retains the measured
intervals, excluding warm-up. Counts are retained from the first measured replay; call and dispatch arrays
are checked for equality against the pilot and across repetitions and samples.
Digests come from the first sample's recording replay. Coarse agreement runs separately, outside
measured intervals.

A digest covers every sink key and every key the tree can write after each tick,
in key-index order, including warm-up. Scale keys follow graph-call order. With
native `scatter`, whose destination names are runtime data, it covers the whole
store, sorted by key name and including those names.
Digests include absent values, boolean tags, UTF-8 strings with their byte
lengths, and little-endian f64 bits, with signed zero and NaNs canonicalized,
using FNV-1a. `differing` counts ticks whose digest
differs from the registered reference, `detached`, which is evaluated even if a
selection omits it. The CLI, Node runner and browser page fail on disagreement.
Library `measure`/`measureAll` calls and the C ABI report `differing`; callers must
check it. Agreement between forms cannot detect a change that moves all forms'
values alike. The cross-build digest check in `analyze.py` catches that, only
within one platform. Hashes are checks for accidental changes, not cryptographic
proofs of equivalence.

The registered forms are:

- `attached-naive`: every graph call is a leaf of a root `PARALLEL` and runs
  every tick. Calls transitively reading a key the tree writes follow the tree;
  other calls precede it, in data order. A reader uses a sink's store key or an
  unpublished call's tree-local return binding.
- `detached`: graph calls are detached nodes referenced by node return ports.
  Sinks run after the tree; a branch's publishing leaves demand its graph calls.
  The executor caches unchanged inputs and evaluates a demanded node at most once
  per tick. This is the agreement reference.

## Workload format

A workload is a JSON replay envelope containing a tree and graph in the
[behavior text language](../arora-behavior-tree/docs/text.md). The text loader
resolves functions and parameters by name against the benchmark's function
index. Any error diagnostic rejects the workload, with the loader's locations
and messages; warnings remain loadable.

| field | meaning |
|---|---|
| `ticks`, `warmUp` | total ticks and the initial ticks excluded from timing; `0 <= warmUp < ticks` |
| `dt` | finite, positive seconds per tick |
| `clock` | the name of a declared key; before tick t the host writes `(t + 1) * dt` |
| `keys` | `[{"name":"input/x","initial":0.0}, ...]`; unique, nonempty store names; names starting with `.` are tree-local and refused |
| `events` | `[[tick,"input/x",scalar], ...]`; known keys, applied in listed order after the clock write |
| `phases` | `{"names":["idle","active"],"runs":[[0,0],[80,1]]}`; nondecreasing starts, phase 0 before the first start |
| `text` | the tree, detached calls and sinks in the behavior text language |

Envelope numbers become f64, booleans and strings retain their kinds, and null
initial values mean absent keys. Text literals use the language's types;
dataflow arithmetic computes in f32. A module's dynamic parameters need typed
numeric literals such as `0.12f32`. Store keys introduced by text without an
initial envelope entry start absent and are included in the digest after the
declared keys, sorted by name. Clock and event keys must be declared.
A tree call binds each store key to at most one output. A key a graph call
writes has no other writer, neither a second graph call nor a tree output:
detached writes graph calls' keys after the tree, in data order, while
attached-naive runs graph calls around the tree in its own order, so a second
writer would leave whichever write the form orders last.

```json
{
  "ticks": 160, "warmUp": 16, "dt": 0.016666666666666666,
  "clock": "arora/time",
  "keys": [{"name":"arora/time","initial":0},
           {"name":"input/x","initial":0},
           {"name":"output/x","initial":null}],
  "events": [[40,"input/x",1]],
  "phases": {"names":["idle","active"],"runs":[[0,0],[40,1]]},
  "text": "running\n\nsmooth: {output/x} = damp(node: \"smooth\", in: {input/x}, param.half_life: 0.12f32, dt: 0.016666667f32, state: ())\n"
}
```

The index is the union of
[`arora-dataflow-nodes::functions::function_index()`](../arora-dataflow-nodes/README.md),
the linked modules in [modules.rs](src/modules.rs), and the host actions below.
The dataflow dependency disables `urdf_ik`, keeping the WASI build plain Rust.
`DataflowNodes::bridge` answers the dataflow calls in process and forwards other
calls to the host bridge. Each replay owns a fresh `DataflowNodes`, including
its per-node caches, state and pending requests. Generated workloads give each
dataflow call its own `node` name so its prepared site can be reused.

Only these host stand-ins are needed:

| action | parameters and behavior | reason |
|---|---|---|
| `hold` | `t`, `duration`, `value`, mutable `out`, mutable `state`; writes once on entry; Running until a positive duration elapses; duration zero stays Running | no workspace function combines entry-only assignment, a status timer and restart after interruption |
| `pulse` | `t`, `gap_min`, `gap_max`, `low`, `high`, `width`, `salt`, mutable `out`, mutable `state`; emits a random level, then zero, after seeded random gaps; Running | oscillators and noise do not schedule random-gap pulses with release deadlines and interruption restart |
| `wander` | `t`, `gap_min`, `gap_max`, `salt`, `low`, `high`, mutable `out`, mutable `state`; emits a random level after seeded random gaps; Running | noise samples do not retain a random-gap schedule and held output across calls |

Host numeric parameters are f64; initialize each private `state` with `()`.
The actions restart after a missed tick (a time gap greater than `1.5 * dt`).
Random choices are deterministic functions of time and salt; [host.rs](src/host.rs)
defines the hash. Outputs are written only when changed.
Assignments and frame selection use native `unpack` and real dataflow calls;
comparisons use `greaterthan` with native `equal`; a branch publishes a graph
value with a one-operand `add` whose return is bound to a key.

The [synthetic workload](src/synthetic.rs) has 21 detached graph calls, two
sinks, real damping, a clock oscillator and four event-driven phases: follow,
pulse/wander, hold/frame selection and set. A root-parallel `say` leaf reads
sentences from `speech/text` and writes `speech/viseme`. Events exercise completed,
interrupted and empty sentences. Native `running` keeps the root Running.
It contains no device data.

## Real modules

The benchmark links workspace functions in process using their declared ids,
names and frozen signatures. `say-silent` supplies `say`, unchanged: `text` and
`voice` are strings, mutable `viseme` is a string, and the return is Status.

```text
parallel
  running
  say(voice: "", text: {speech/text}, viseme: {speech/viseme})
```

The text loader owns binding and literal checks. Module parameters that are not
bound follow the language's absent-argument semantics. Both tree and detached
calls can use linked functions. Mutable outputs pass through unfiltered,
including unchanged values, as the production module contract requires.

To add a module, add its workspace path dependency and an entry in
[modules.rs](src/modules.rs)'s `MODULES`, using the declaration's module id,
exports function and reset function. Every replay resets the registered modules
and holds one process-wide lock through lowering and ticking, since module state
can be global. The bridge calls `invoke` directly, without engine serialization
or transport; borrowed calls to these exports copy their arguments.

The counting observer is reinstalled by a proxy around every registered
callable, since the dataflow bridge invokes its callables through itself.
Dataflow calls count at that observer; host and linked calls count where answered.
Forwarding adds no second count. Every `arora_call_indirect` counts once, including
the root and natives. The hand-derived mixed-call test in
[agreement.rs](tests/agreement.rs) checks both forms.

## Scale scenarios

Default sizes are 10, 30, 100, 300 and 1000 graph calls. Scenarios generate text
and pass through the same loader, host, forms and measurement code as JSON files.

- `settled`, `one-input`, `all-inputs`: K=max(2,N/10) independent chains of real
  add, multiply, clamp and remap calls, chosen by depth. Depth is ten for N≥30
  at the default sizes, five at N=10. Each chain ends in a sink. Inputs remain
  constant, input 0 changes, or all inputs change to `tick % 17`. Settled work
  falls to zero; one-input work depends on depth; all-inputs work grows with K.
  Unchanged intermediate values stop propagation.
- `clock`: the same chains, with an oscillator reading the clock at the head of
  each chain. Work grows with K and depth; clamping can stop propagation.
- `branches`, `branches-switching`: eight graphs of ceil(N/8) real calls guarded
  by native equality checks under a fallback. Every input changes each tick.
  One branch stays active or the active branch changes every 50 ticks. Only the
  active branch publishes. Branch graphs use add, multiply, clamp, remap and damp.

Branch topology uses a u32 LCG: seed 332396, multiplier 1664525, increment
1013904223. Each graph has max(2,n/10) inputs. Calls read an input or a previous
call within 32 positions. Unread calls publish, with at least ceil(n/10) published
calls. Branch sizes can round above N. Every scenario times the host clock write.

Switching uses one-operand add in place of damping, preserving graph edges.
Continuously running inactive smoothers and pausing them have different reentry
semantics; stateless switching isolates branch scheduling and preserves agreement.
Fixed branches retain damping, and synthetic exercises smoothing and timed actions.

Calls per measured tick with 200 warm-up and 500 measured ticks:

| form / scenario | N=10 | N=30 | N=100 | N=300 | N=1000 |
|---|---:|---:|---:|---:|---:|
| detached / settled | 0 | 0 | 0 | 0 | 0 |
| detached / one-input | 3.240 | 3.840 | 3.840 | 3.840 | 3.840 |
| detached / all-inputs | 6.480 | 11.520 | 38.400 | 115.200 | 384.000 |
| detached / clock | 7.044 | 10.566 | 35.220 | 105.660 | 352.200 |
| detached / branches | 4.000 | 6.000 | 18.000 | 58.000 | 188.000 |
| detached / branches-switching | 4.000 | 6.500 | 19.800 | 59.400 | 188.900 |
| attached-naive / settled, one-input, all-inputs, clock | 10 | 30 | 100 | 300 | 1000 |
| attached-naive / branches | 18.000 | 34.000 | 109.000 | 324.000 | 1063.000 |
| attached-naive / branches-switching | 18.000 | 34.500 | 110.800 | 325.400 | 1063.900 |

Detached runs demanded calls in the active branch plus its publishing leaves;
attached-naive runs every graph call plus active publishing leaves.

## Adding a form

1. Add a module under [src/forms](src/forms) implementing `Form`.
2. Implement `build(&Workload) -> Nodes`. The workload holds the text loader's
   schema nodes, variable names and attached-node ids. Transform them in the
   form's module; detached returns them unchanged.
3. Override `lower` when needed. It receives nodes, names, the variable resolver,
   function index and mutable bridge. The resolver is shared and owned so native
   `scatter` can resolve runtime keys. A placement pass can run before loading;
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
cargo build --profile bench-wasm -p arora-behavior-tree-bench --target wasm32-wasip1 --lib
cargo run --release -p arora-behavior-tree-bench --bin synthetic-workload > target/bench-synthetic.json
node crates/arora-behavior-tree-bench/js/run-node.mjs \
  target/wasm32-wasip1/bench-wasm/arora_behavior_tree_bench.wasm target/bench-synthetic.json
# Test-only wrong form and virtual clock, excluded from the distributable module:
cargo build --profile bench-wasm -p arora-behavior-tree-bench --target wasm32-wasip1 --lib --features bench-check
node crates/arora-behavior-tree-bench/js/check.mjs \
  target/wasm32-wasip1/bench-wasm/arora_behavior_tree_bench.wasm target/bench-synthetic.json
# Rebuild without test fixtures before running measurements:
cargo build --profile bench-wasm -p arora-behavior-tree-bench --target wasm32-wasip1 --lib
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
if (Object.values(result.forms).some(r => r.differing)) throw new Error('forms disagree');
const scale = await bench.loadScale(100, 'clock').measureAll({ samples: 7 });
if (Object.values(scale.forms).some(r => r.differing)) throw new Error('scale forms disagree');
```

Only one workload/scale session is live per module instance. Loading another
replaces the previous session. Calls are synchronous; use a Worker for an
interactive page during long replays.

The loader measures the smallest positive `performance.now()` step across 32
clock transitions and passes it to the module. `clock.stepUs`,
`clock.minimumSampleUs`, `clock.modes` and `crossOriginIsolated` accompany each
record. The pilot selects tick or replay timing and fixes repetitions before
measurement, as described above. Isolation usually improves browser precision;
repeating individual tick timings cannot recover sub-step latency quantiles.
Calibration costs at most 32 steps on usual clocks; a frozen clock is rejected.
The module exports scale defaults, the reference name and scale test names so JS
uses the same definitions as native.

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
| `minimum_sample_us(us)` | nonnegative pilot target per sample; 0 disables repetition |
| `clock_step_us(us)` | nonnegative clock step used to select timing mode |
| `defaults()`, `defaults_len()` | borrowed UTF-8 JSON: ticks, warmUp, sizes, reference |
| `scale_name(n,scenario)` | writes a JSON scale test name to result; status 0 on success |

The documented WASM build uses `bench-wasm`, inheriting release optimization
with debug info disabled and symbols stripped. The workspace release profile
continues to keep debug info for other builds. Record profile overrides and
toolchain changes alongside comparisons. `bench-check` is a fixture feature for
`check.mjs`, adding a deliberately wrong form and virtual-clock export; leave it
disabled for measurements.

## Comparison protocol

```sh
ROUNDS=8 BENCH_FORMS=detached,attached-naive \
  crates/arora-behavior-tree-bench/scripts/run.sh /tmp/bench-results \
  base=<revision> candidate=<revision> another=<revision> -- /path/workload.json
python3 crates/arora-behavior-tree-bench/scripts/analyze.py /tmp/bench-results \
  --baseline base --forms detached,attached-naive
```

Revisions must contain this crate and the chosen forms. The Bash runner creates
one detached worktree and target directory per revision, builds the round binary
with `--release --locked`, and runs one process per build per round. Extra cargo
flags are whitespace-separated in `BENCH_CARGO_FLAGS` (for example, `--offline`).
The default is eight rounds; the runner requires an even count of at least four.
Order is forward on odd rounds and reversed on even rounds. One discarded round
runs each build once before the first: the round straight after the builds can
run on a machine still settling from compiling. Exit traps remove
created worktrees on success, failure and interruption; target directories,
resolved revision IDs, raw rounds and comparisons stay under the output directory.
Use a new directory for each run. Compiler settings may be supplied through the
invoking environment. Analysis also requires at least four rounds and rejects odd
counts unless `--allow-odd-rounds` explicitly accepts unbalanced order.

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
- **Retain clock metadata.** Coarse measurements resolve replay means; their
  per-tick latency statistics remain unresolved.

## A/A run

Run the same revision under two build labels to measure this machine's noise
floor before trusting a difference:

```sh
ROUNDS=8 crates/arora-behavior-tree-bench/scripts/run.sh /tmp/bench-aa \
  a=HEAD b=HEAD -- /path/workload.json
```

A whole process can be an outlier because of startup state, scheduling or thermal
conditions. Rounds use separate processes, after a discarded real-work pilot, and
intervals come from bootstrapping rounds together across all tests in a process.
Other work on the machine during a run, a build included, widens every interval
and can load one build's rounds more than the other's, so run comparisons on an
otherwise idle machine. The short synthetic workload is the noisiest test; a
longer workload of your own resolves smaller differences.
An A/A interval measures session noise, not a universal threshold for improvement.

## Checks

```sh
cargo test --release -p arora-behavior-tree-bench
cargo clippy -p arora-behavior-tree-bench --all-targets -- -D warnings
cargo fmt -p arora-behavior-tree-bench --check
cargo bench -p arora-behavior-tree-bench --bench workload --bench scale --no-run
python3 -B crates/arora-behavior-tree-bench/scripts/test_analyze.py
python3 -B crates/arora-behavior-tree-bench/scripts/test_run.py
```

The registry test runs synthetic plus N=10 of every scenario, compares every tick
with detached and asserts each form's total module calls and dispatches. The
Python checks run under `cargo test` as well, including runner lifecycle checks
using local Git and Cargo stand-ins that do not modify a repository. Negative
agreement fixtures and a quantized virtual clock check the failure paths. The existing CI package selection
includes this default member and its tests and lint targets. WASI/Node execution
is a separate local check; there is no additional WASI CI job.
