# Testing

How to run the tests and what they build, the tests that need a feature, a
browser or a machine of their own, what CI runs, how to measure coverage, and
the rules that keep the suite free of tests that pin nothing new. What to
install and how the workspace builds is [`building.md`](building.md).

## Running the tests

```bash
cargo test
```

This runs the unit, integration and doc tests of the default members — the
crates a bare cargo command acts on ([`building.md`](building.md#build)). It
builds what those tests load on its own:

- the Rust WebAssembly guests, compiled for `wasm32-wasip2` as artifact
  dependencies of the crates whose tests load them;
- the C++ guests `test-cpp`, `test-cpp-2` and `example-cpp`, through the
  dev-dependencies of the integration tests: their build scripts write each
  guest's module directory to `target/<profile>/modules/<name>/`. This is the
  step that needs CMake, and it downloads the WASI SDK on the first run
  ([`building.md`](building.md#the-wasi-sdk)).

CI runs the same tests optimised, so wasmtime runs the guests at speed, without
the release profile's thin LTO and with line tables only: every test binary
links wasmtime, and LTO over each of them would be most of the build. To run
them as CI does:

```bash
CARGO_PROFILE_RELEASE_LTO=false CARGO_PROFILE_RELEASE_DEBUG=line-tables-only cargo test --release
```

Narrower runs:

```bash
cargo test -p arora-hal                                                    # one crate
cargo test -p arora-integration-tests --test component                     # one test file
cargo test -p arora-behavior-tree graph::tests::groot_lowers_to_graph_and_runs   # one test
cargo test -p test-rust-wasm                                               # a member outside the default set, on the host
cargo test -p arora-hal -- --nocapture                                     # with the tests' output
```

`cargo test --workspace` adds the members outside the default set: the test
guests' own unit tests, which the default run does not reach (the integration
tests build those guests only as artifacts), and `polly`'s, which stay
ignored.

### Tests that do not run by default

| Tests | Why | To run them |
| --- | --- | --- |
| The live DDS tests: `arora-hal-ros2`'s in `src/ros2_hal.rs` and `src/configs/nao.rs`, `arora-bridge-ros2`'s `tests/tests.rs`, `tests/device.rs` and `tests/refusal_log.rs` | Ignored on macOS, where DDS discovery over multicast is unreliable on loopback. They run on Linux, CI included. | `--ignored`, on a machine with an active multicast-capable interface |
| `crates/arora-bridge-ros2/tests/zenoh_native.rs` | Needs Docker and an image with ROS 2 Jazzy and `rmw_zenoh`; the file's documentation shows one. | `ROS2_ZENOH_IMAGE=<image> cargo test -p arora-bridge-ros2 --no-default-features --features zenoh --test zenoh_native -- --ignored` |
| `modules/polly/tests/polly_tests.rs` | Calls AWS Polly. | `cargo test -p polly -- --ignored`, with AWS credentials |
| `arora`'s `module_reload_tests::measure` (macOS) | A measurement of reload cost, not a check. | `cargo test -p arora --lib module_reload_tests::measure -- --ignored --nocapture --test-threads=1` |

## The integration tests

`arora-integration-tests` ([`tests/`](../tests/Cargo.toml)) drives built
modules end to end, through `arora-cli`, the engine and the device runtime. Its
build script needs nothing built beforehand; the tests find their inputs three
ways:

- **The Rust guests** — `test-rust-wasm`, `test-rust-component`,
  `test-behavior-tree-nodes` and the animation module — are artifact
  dependencies of its build script, built for `wasm32-wasip2`.
  [`tests/build.rs`](../tests/build.rs) forwards their paths as
  `CARGO_CDYLIB_FILE_*`.
- **`arora-cli`** is a binary artifact dependency; `tests/build.rs` forwards its
  path as `ARORA_CLI_BIN`.
- **The C++ guests** are plain dev-dependencies. Their build scripts write their
  module directories under `target/<profile>/modules/`, and the tests read
  `target/debug/` or `target/release/` according to `debug_assertions`.

| File | What it holds |
| --- | --- |
| `integration.rs` | `arora-cli` loads a guest from its header and calls it: `test-rust-wasm`, whose header comes from its Rust declaration; `test-cpp`'s optional parameters; `test-cpp-2` calling into `test-cpp` with a structure. |
| `component.rs` | The component executor, in process, over `test-rust-component` and `test-rust-wasm`: calls, callbacks through `dispatch-indirect`, the WASI interfaces, traps, argument checks, call budgets and memory caps. |
| `behavior_tree_nodes.rs` | `test-behavior-tree-nodes`, a Rust guest generated from a `module.yaml` by `arora-module-rust`: its exports serve calls, and a control node ticks its children through the `host` interface. |
| `cpp_module_dirs.rs` | A device given the C++ module directories describes their exports from their records and calls them; `example-cpp` keeps its state across calls; a C++ guest refuses an argument that is not an Arora buffer, and is never handed one of another type. |
| `guest_parity.rs` | The C++ and Rust guests have one form: a component of the `arora:module` world importing only what every host provides. |
| `animation.rs` | The animation guest and the host-linked module answer the same calls; a runner given the guest's module directory describes its exports; the records `@vizij/animation-module` publishes load beside the module's own ([`fixtures`](../tests/fixtures/vizij-animation-module-4.1.0/README.md)). |

## Feature builds and the browser

The default run builds every crate with its default features. The code behind
the other features, and the code that compiles only for `wasm32`, has its own
commands, which CI runs in jobs of their own
([`continuous.yml`](../.github/workflows/continuous.yml)).

### The Zenoh backend

The ROS 2 crates build with DDS by default, and their Zenoh backend excludes
it:

```bash
cargo test --release -p arora-bridge-ros2 -p arora-msgs-ros2 --no-default-features --features zenoh
```

### Robot runners and Semio Studio

The `arora-ros2` and `arora-restful` runners build only with their crates'
`runner` feature, and connect to Semio Studio only with `studio`, which turns
on `arora`'s `studio-bridge`:

```bash
cargo test -p arora-hal-ros2 -p arora-hal-restful --features arora-hal-ros2/runner,arora-hal-restful/runner --bins
cargo test -p arora-hal-ros2 -p arora-hal-restful --features arora-hal-ros2/runner,arora-hal-restful/runner,arora-hal-ros2/studio,arora-hal-restful/studio --bins
cargo test -p arora-hal-ros2 -p arora-hal-restful --features arora-hal-ros2/runner,arora-hal-restful/runner,arora-hal-restful/studio --bins
cargo test -p arora --features studio-bridge --lib -- studio:: run::
```

The third builds the two runners together with `studio` on one of them. Cargo
unifies features across the packages one command builds, so `arora` carries
`studio-bridge` for both, and the runner without `studio` must still take no
Studio option, refusing `--no-studio`.

### Browser tests

The engine's browser executor, the run loop's JavaScript-timer cadence and
`arora-web` compile only for `wasm32-unknown-unknown`, and run in a headless
Firefox through `wasm-pack test`. They need the `wasm32-unknown-unknown`
target, `wasm-pack`, Firefox and geckodriver
([`building.md`](building.md#the-browser-runtime)):

```bash
wasm-pack test --headless --firefox crates/arora-engine --no-default-features --test browser_component
wasm-pack test --headless --firefox crates/arora --no-default-features --lib
wasm-pack test --headless --firefox crates/arora-web --lib --test browser
```

`wasm-pack`'s own flags come before the crate's path, cargo's after it. The
guests these tests load are artifact dependencies, built along with them.

On Apple silicon, the geckodriver `wasm-pack` downloads is an x86_64 build that
aborts under Rosetta: install a native one (`brew install geckodriver`) and
point `GECKODRIVER` at it, as in
`GECKODRIVER=$(which geckodriver) wasm-pack test --headless --firefox …`.

## What CI runs

[`.github/workflows/continuous.yml`](../.github/workflows/continuous.yml) runs
on every pull request and every push to `main`, as parallel jobs. A pull
request builds and tests only the members its diff can reach; `main` runs
everything.

### Which members a change reaches

[`.github/scripts/list_modified_targets.sh`](../.github/scripts/list_modified_targets.sh)
makes the selection:

- A changed file selects the member whose directory holds it, and the members
  the script's `INPUTS` lists as reading it.
- A file that no `INPUTS` line names and the script's `IGNORE` matches selects
  nothing: Markdown anywhere, `docs/`, `examples/policy-modules/`,
  `release.yml` and a few more.
- Any other file in no member — `Cargo.toml`, `.cargo/`, `rust-toolchain.toml`,
  CI — selects everything.
- A change to `Cargo.lock` selects the packages whose entry changed.
- The selection spreads through cargo's dependency graph: along normal, build
  and artifact edges to everything that depends on a selected package, and one
  hop along dev-dependency edges.

To see what a branch selects:

```bash
.github/scripts/list_modified_targets.sh origin/main
```

After the `test` job's build, `list_modified_targets.sh verify` checks every
input cargo recorded — the sources rustc read, and the files build scripts
declared with `cargo:rerun-if-changed` — against those rules, and fails on a
file whose change would not select the package that read it. A package that
reads a file outside its own directory (a build script reading `libs/cpp`,
`wit_bindgen` reading the engine's WIT world) declares it with
`cargo:rerun-if-changed` and has its line in `INPUTS`; the failure names the
line to add.

### The jobs

| Job | Runs when the selection holds | What it runs |
| --- | --- | --- |
| `select` | always | The selection above. |
| `test` | a default member | `cargo test --release` over the selected default members, as [above](#running-the-tests), then `verify`. |
| `zenoh` | `arora-bridge-ros2` or `arora-msgs-ros2` | [The Zenoh backend](#the-zenoh-backend). |
| `runners` | `arora`, `arora-hal-ros2` or `arora-hal-restful` | [The runners](#robot-runners-and-semio-studio), with clippy over each feature set. |
| `lint` | a default member, or `polly` | `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` over the selected default members; the same over `polly` when it is selected. |
| `browser` | `arora-engine`, `arora` or `arora-web` | [The browser tests](#browser-tests), and `cargo build -p arora-web --target wasm32-unknown-unknown`. |
| `coverage` | pushes to `main` only | [Coverage](#coverage). |
| `markdown-link-check` | always | Checks the links of every Markdown file. |

`select` has ten minutes, every Rust job forty. The Rust jobs restore a cache of
the dependencies, which only `main` saves. A pull request that changes only
files `IGNORE` matches runs `select` and `markdown-link-check` alone.

To check a page's links as CI does:

```bash
npx markdown-link-check@3.13.7 --quiet --config .github/workflows/markdown-link-check-config.json docs/testing.md
```

## Coverage

[`cargo-llvm-cov`](https://github.com/taiki-e/cargo-llvm-cov) measures line and
region coverage of the native tests. Install it once:

```bash
cargo install cargo-llvm-cov          # or: brew install cargo-llvm-cov
rustup component add llvm-tools-preview
```

Then, from the workspace root:

```bash
host=$(rustc -vV | sed -n 's/^host: //p')
cargo llvm-cov --release --target "$host" --coverage-target-only --html          # target/llvm-cov/html/index.html
cargo llvm-cov --release --target "$host" --coverage-target-only -p arora-hal --summary-only
```

`--target "$host" --coverage-target-only` instruments the host target alone.
Without it the instrumentation reaches the wasm32 guests the tests build as
artifact dependencies, and those cannot link the profiler runtime. `main` runs
the same measurement in CI's `coverage` job: the totals and the per-file table
are in the job summary, `lcov.info` in its artifact. It gates nothing.

What native coverage cannot see, so a low number there is not a gap in the
tests:

- **wasm32 code**: the guests the integration tests load, and the browser host
  (`executor::browser`, `arora-web`), which the `browser` job tests in Firefox.
- **Other processes and the compiler**: `arora-cli` as the integration tests
  spawn it (a host build dependency, built uninstrumented), the C++ modules,
  the code generators run by build scripts, and the proc macros as trybuild
  drives them.
- **Derived code**: what `serde`, `thiserror` and similar derives expand to
  carries no source line, so a test of a wire format shows as covering nothing.
- **Code behind a feature or a platform**: the Zenoh backend (its own job), the
  `#[ignore]`d live tests, and the DDS tests, which are ignored on macOS.

## Adding a test

- **Test at the lowest level that can observe the behaviour.** A unit test
  names the behaviour, fails next to its cause and runs in microseconds; an
  end-to-end test adds a process, a socket or a ROS graph to every failure.
- **One end-to-end path per bridge or feature**, for what only the assembled
  system shows: the wiring between the parts. These are the paths:

  | path | file |
  | --- | --- |
  | the local WebSocket bridge over a running device | `crates/arora/tests/local_ws_bridge.rs` |
  | the WebSocket wire against a fake runtime | `crates/arora-bridge-ws/tests/wire.rs` |
  | the ROS 2 bridge over live DDS | `crates/arora-bridge-ros2/tests/tests.rs` |
  | the ROS 2 bridge over a running device, on both backends | `crates/arora-bridge-ros2/tests/device.rs` |
  | the ROS 2 bridge to a native subscriber (`#[ignore]`d, Docker) | `crates/arora-bridge-ros2/tests/zenoh_native.rs` |
  | guests through `arora-cli` | `tests/tests/integration.rs` |
  | a device loading module directories | `tests/tests/cpp_module_dirs.rs`, `tests/tests/animation.rs` |

  A variant of a behaviour that a unit test can reach — another input, another
  error — belongs in a unit test, not in another end-to-end case.
- **A test pins something no other test does.** Before adding one, compare the
  crate's coverage with and without it:

  ```bash
  cargo llvm-cov --release --target "$host" --coverage-target-only -p <crate> --summary-only -- --skip <test>
  cargo llvm-cov --release --target "$host" --coverage-target-only -p <crate> --summary-only
  ```

  When it covers no region the others miss, what it adds is its assertion — a
  different input's outcome, an error's message — and its doc comment says
  which. Two tests that drive the same code with the same input and assert the
  same outcome are one test too many.
- **Bound every wait.** A test that waits for an event — a sample, a message, a
  connection — waits under a timeout. An unbounded wait turns a missed event
  into a CI job cancelled at its time limit, with no failure to read.
- **A slow test needs a reason.** Spawning a process, compiling, or a real
  network makes a test slow; anything else that takes over a second is a
  problem to fix, not to accept.
- **A test that needs a guest takes it as an artifact dependency**, built for
  the target it loads, and reads its path with `env!`, as
  [`building.md`](building.md#artifact-dependencies) describes. A C++ guest is
  the exception: its module directory is read from `target/<profile>/modules/`.
