# Testing

How to run the tests, how to measure what they cover, and the rules that keep
the suite free of tests that pin nothing new.

## Running the tests

```bash
cargo test --release                  # what CI's `test` job runs, over the default members
cargo test --release -p arora-hal     # one crate
```

Never pass `--workspace`: it adds the opt-in NAO cross-compile, which needs its
own toolchain. On a pull request CI tests only the members the diff reaches;
`.github/scripts/list_modified_targets.sh origin/main` prints that selection.
The Zenoh backend of the ROS 2 bridge and the wasm32 browser tests run in their
own CI jobs (see [the CI section of the architecture](architecture.md#ci)); the
commands are in `.github/workflows/continuous.yml`.

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
  | the ROS 2 bridge to a native subscriber (`#[ignore]`d, Docker) | `crates/arora-bridge-ros2/tests/zenoh_native.rs` |
  | the NAOqi device against a fake NAOqi | `crates/arora-hal-naoqi/tests/device.rs` |
  | guests through `arora-cli` | `tests/tests/integration.rs` |
  | asset resolution and fetch through a local directory source | `crates/arora-index/tests/resolver.rs` |
  | exact public models built through a local directory source, then an offline rebuild | `crates/arora-index/tests/build.rs` |

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
