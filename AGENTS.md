# Agent guide

What an agent needs to work in this workspace without breaking it. Read, in
order:

1. [`readme.md`](readme.md) — what Arora is, and the crate map;
2. [`docs/overview.md`](docs/overview.md) — the concepts, one at a time;
3. [`docs/architecture.md`](docs/architecture.md) — the crates' layers, the
   device, the engine, the module pipelines;
4. the readme of the crate you are changing; then
   [`docs/design_decisions.md`](docs/design_decisions.md) before changing a
   rule it records.

## Build and test

The workspace pins a nightly toolchain (`rust-toolchain.toml`) because it uses
cargo's artifact dependencies (`[unstable] bindeps = true` in
`.cargo/config.toml`). [`docs/building.md`](docs/building.md) lists the
prerequisites (CMake ≥ 3.20, a host C/C++ compiler; the WASI SDK downloads
itself).

```bash
cargo build                         # the default members
cargo test                          # default members' tests; builds the guests the tests load
cargo test -p arora-behavior-tree   # one crate
cargo test -p arora-behavior-tree graph::tests::groot_lowers_to_graph_and_runs
```

- `--workspace` adds the opt-in members (`polly`, `arora-web`, the test
  guests, the C++ crates); it is slow, and rarely what a change needs.
- Rust guests build for the host by default; their `wasm32-wasip2`
  component is built on demand by the crates that depend on them as
  artifacts, so `cargo test` needs no `--target`.
- A build script's output is shown only on failure or with `-vv`; it is kept in
  `target/<profile>/build/<package>-<hash>/output`.
- Artifact paths reach build scripts as environment variables. For a
  dash-named staticlib or cdylib, cargo sets only
  `CARGO_<KIND>_FILE_<DEP>_<lib>` (dashes → underscores) and
  `CARGO_<KIND>_DIR_<DEP>`, never the bare `CARGO_<KIND>_FILE_<DEP>`
  ([why](docs/design_decisions.md#-z-bindeps-artifact-dependencies-over-recursive-cargo)).

[`docs/testing.md`](docs/testing.md) covers the integration tests, the browser
and Zenoh runs, coverage, and the rules for adding a test.

Cargo traps that cost hours when unknown:

- `[patch.crates-io]` is silently ignored when the patched crate's version
  does not satisfy the requirement; raise the requirement first. Patch a
  crate's whole dependency closure, never one crate of it: a lone patched
  crate pulls path copies of its dependencies beside the registry ones
  ("multiple versions of crate `arora_bridge`"). A temporary patch goes in
  `.cargo/config.toml`, not `--config`: `arora-buffers`' build script runs
  `cargo metadata`, which does not see a `--config` patch.
- A git dependency on these crates needs `[unstable] bindeps` (nightly) just
  to parse their manifests, because some carry `artifact =` dependencies;
  without it cargo reports "no matching package". crates.io consumers are
  unaffected.
- On `wasm32-unknown-unknown`, tokio compiles only with `sync`, `macros`,
  `io-util`, `rt` and `time`. Never `tokio = { features = ["full"] }` in the
  runtime's graph; use `std::fs` for startup and code-generation I/O.
- A `[target.'cfg(…)'.build-dependencies]` gate is evaluated against the
  host, so it cannot keep a build dependency out of a wasm build: make the
  dependency optional behind a feature.
- Cargo applies no LTO to a lib whose `crate-type` includes `rlib`, even with
  `lto = true`. When a smaller wasm matters, build a `cdylib`-only crate;
  `wasm-opt --strip-debug` is the larger saving either way.

### What CI runs on a pull request

CI builds and tests only the members the diff can reach
([`list_modified_targets.sh`](.github/scripts/list_modified_targets.sh)). A
package that reads a file outside its own directory (a build script reading
`libs/cpp`, `wit_bindgen` reading the engine's WIT world) prints
`cargo:rerun-if-changed` for it and has a line in the script's `INPUTS`;
CI's `verify` step fails otherwise, naming the line to add.

```bash
.github/scripts/list_modified_targets.sh origin/main   # what this branch selects
```

## Where a module's interface lives

A module's interface — its id, its functions, their parameters, each with its
UUID — has exactly one source. Find it before editing anything:

- **A Rust declaration** (`modules/polly`, `modules/test-rust-wasm`,
  `modules/animation`): the [`arora-module`](crates/arora-module/readme.md)
  macros on a Rust module (`#[arora_module::module]`, `#[export]`,
  `#[param]`). There is no `module.yaml`, no build script and no generated
  source: the header, the records, the host registration, the client stubs
  and the entry points come from macro expansion. Edit the Rust code.
- **A `module.yaml`** beside `Cargo.toml`: C++ modules, and Rust modules that
  call other modules' functions, which a declaration cannot express
  (`modules/test-behavior-tree-nodes`). The YAML is the source; the bindings
  are generated from it on every build.

### A Rust module

1. Depend on `arora-module`.
2. Put `#[arora_module::module(id = "…", name = "…", version = "…")]` on an
   inline Rust module, `#[export(id = "…")]` on each function and
   `#[param(id = "…")]` on each parameter. Ids are written in full, never
   derived from names.
3. Write `arora_module::artifact!(<module>)` once, at the root of the crate
   built as the artifact: for `wasm32-wasip2` it is a component of the
   `arora:module` world; for the host, a shared library.
4. A host links it with `HostModule::of::<M>()`; a device loads its module
   directory.

Functions several modules implement, each under its own module id, are a
contract (`modules/animation`'s `AnimationContract`): `#[arora_module::contract]` on a trait whose
methods take `&mut self`, registered with
`HostModule::from_exports(module_id, contract::exports(implementation))`.

### A `module.yaml` module

- `exports:` lists the functions it serves, `imports:` the functions of other
  modules it calls. Dependencies are derived from `imports:`; there is no
  `dependencies:` key to fill.
- The generators read the imported modules' records:
  - a C++ module's `CMakeLists.txt` passes them to `arora-module-cli` with
    `--include` (`modules/test-cpp-2/CMakeLists.txt`);
  - a Rust one's `build.rs` adds them to a `LocalRegistry` before
    `analyze_module_from_path` (`modules/test-behavior-tree-nodes/build.rs`).
- The build scripts print `cargo:rerun-if-changed=module.yaml`: a changed
  YAML regenerates on the next build, with no `cargo clean`.
- Generated sources are never edited. A Rust module's land in
  `src/arora_generated/`; a C++ module's in its CMake build tree under
  `OUT_DIR`.
- A C++ guest links `arora-cpp-component` (`libs/cpp`), which makes it a
  component of the `arora:module` world, and `arora_module_directory` writes
  its module directory. C++ guests call other modules with `arora_dispatch`;
  they have no indirect dispatch. See
  [`examples/modules/cpp`](examples/modules/cpp/readme.md).

## Common failures

| Symptom | Cause | Fix |
|---|---|---|
| `function <uuid> is missing from index` | a tree names a function no loaded module exports | find the module that should export it (check the id against its declaration), export it, and make sure the device or test loads that module |
| `no such record "<uuid>"` during a build | the generator was not given the record of a type or module the `module.yaml` names | pass that module's records (`--include` in CMake, or the `LocalRegistry` in `build.rs`) |
| an edit to `src/arora_generated/` disappears | generated sources are rebuilt from `module.yaml` | edit the YAML |
| a call fails on its arguments | an id mismatch between caller and declaration | compare the `#[export]`/`#[param]` ids, or the YAML's, with the ids the caller uses |

## Writing in this repository

Docs, comments and commit messages follow the same rules:

- **State what is true now, and why.** History belongs in git: no "was",
  "no longer", "we first tried", no attribution, no incident counts.
  Alternatives that were not chosen go in `docs/design_decisions.md`.
- **Vocabulary.** A key is a `/`-separated path; a trailing `.attribute` is a
  naming convention only. A module never writes keys: a call has in and out
  parameters, and a behavior or a remote client writes outputs to keys. A key
  is closed to remote writers unless its meta opens it. "Registry" means the
  type registry. A keyframed asset is an "animation". A module's name has no
  `-module` suffix (the module `@arora/animation`); the crate shipping it does
  (`arora-animation-module`). Say "wire format" and "messages", never "Arora
  protocol"; "slot" means `arora_types::data::Slot` only. A task run starts
  with `spawn` and stops with `halt` (`drop` overloads Rust's `Drop`, `abort`
  collides with ROS's `ABORTED`). Never "io pump". Before naming a public
  item, search the existing vocabulary of Arora, Vizij and Studio and reuse
  it.
- **Semio Studio is opt-in.** Nothing in Arora connects to Studio by default:
  the connection sits behind the `studio-bridge` feature of `arora` and each
  runner's `studio` feature.
- **Write a comment for the code's client**, not for the discussion that
  produced it: what the item does, its effects, and the "why" that stops a
  reader from deleting something that looks dead. An item defines itself by
  what it does, never by its callers. Document a private item as if it were
  public; document public API exhaustively, because it is a contract, and keep
  internal comments lean.
- **Something awkward to explain is a design smell.** Raise it instead of
  writing around it.
- Every relative link must resolve; CI link-checks every Markdown file.

## Design rules

These constrain every change. Each has its reason, so it can be applied to a
case it does not name.

- **Arora stands alone.** It builds and runs without Studio and without Vizij:
  Vizij's nodes and modules (its speech providers included) live in vizij-rs,
  and Studio's store, credentials and caches live in studio-bridge. Reuse
  Arora's own concepts (records, `arora-registry`) before adding vocabulary.
- **A bridge exposes the device by itself.** Methods come from the modules'
  declarations (`DescribeMethods`, `Call`), keys and their meta from the store
  (`KeyMeta`). An app never restates its device to a bridge and a bridge never
  filters the device's keys: what one app restates is restated in every app
  and lost on every other bridge. A client controls volume by choosing the
  keys it subscribes to. A bridge's own state is conversion only (ROS 2
  profiles).
- **Opening keys to remote writers is explicit**, and easy by subtree (the
  empty prefix opens everything); nothing is open by default, because an open
  key lets any connected client drive the device. Check every
  `#[derive(Default)]` on a struct whose default must be the closed one.
- **Features go through a module's declared functions.** What a device holds
  (animations, programs) is loaded and unloaded through the calls any client
  sends, at any time, and the resulting state is observable through the
  module. A real device gains resources at run time from clients that never
  saw its build, so a construction-time list or a host-only method leaves
  them blind. Declare a missing inverse (`unload`, `remove`) instead of adding
  a side door.
- **A client library adds no function Arora already provides** without a
  written reason: each wrapper is a second API that remote clients lack, and
  it drifts. When a host lacks an operation remote clients have, add it to
  Arora.
- **No JSON on the value plane.** Values, signatures and handles travel as
  `Value`s (`value_serde`); a ROS bridge speaks real ROS messages, never a
  JSON-wrapping service.
- **A ROS HAL is typed and builds on both backends**: generated message
  structs, typed publishers, the `dds` and `zenoh` features. String-dispatched
  `Value`/CDR publishing loses the build-time checks and ties the HAL to one
  middleware.
- **Never change a standard ROS interface type**, even with an added field.
  Add a distinct interface, and document which one a client chooses when two
  drive the same thing. A type is matched by its name and, from Jazzy, by a
  hash of its fields: an extended type under the standard name fails to match
  upstream peers or mis-decodes.
- **The HAL carries actuation.** A simulated sensor reading is a store write,
  not a HAL path. The device binary runs the same on the robot and in
  simulation; only the HAL differs.
- **The runtime holds no mutex and spawns nothing it does not need.** An
  async module function is polled on the tick
  ([`docs/async-functions.md`](docs/async-functions.md)). Stopping a run is
  dropping its future: there is no `run_until` or `with_shutdown`.
- **A library never reads `argv`.** The binary parses its arguments (a clap
  helper in the library is fine), and Arora is not patched for one app's
  command line. A feature is named for the concept it gates, not for the
  consumer that wants it.
- **`arora-buffers`' wire format is discussed before it changes.**
- **`module.yaml` and the generators are a product path**: modules written in
  other languages, and records published to a registry. A new type-model form
  lands in the Rust and C++ generators and in the header ↔ record conversion
  in the same change, with tests.
- **Generic means indifferent.** A type parameter depends on the minimal
  interface the code needs; a trait restating one concrete type's whole
  vocabulary is not generic.
- **Fix a cross-platform problem at the layer that makes platforms uniform.**
  A platform-specific mitigation is a labelled stopgap, agreed before it
  lands, because per-platform patches multiply and entrench the wrong design.
- **Private by default.** An item becomes `pub(crate)` or `pub` when a real
  caller needs it, never to test it: a public item is a contract whose removal
  is a major. A design that must expose a lot is a smell to raise.
- **DRY, without factoring ahead of need.** Extract on real redundancy or a
  function grown very long: every extraction is indirection paid on each
  read. Name an extracted free function for what it does; code reused within
  one function becomes a closure.
- **Measure performance in instructions, not wall time** (callgrind's `Ir`,
  `ri_instructions` from `proc_pid_rusage`, `time -l`'s "instructions
  retired"). Builds and agents share the machines, so timings there are noise.

## Versions and releases

[`docs/building.md`](docs/building.md#releasing) has the mechanics; these are
the rules.

- **A breaking change bumps the real major**, below 1.0 too (`0.4` → `1.0.0` →
  `2.0.0`): cargo's 0.x-minor convention is not used, so "we broke the API"
  always reads as a major. Dependents re-pin in the same change, and a
  dependent whose API carries the changed types bumps its own major.
- **Before a major, list who can hit the break.** A change no consumer can
  hit is not one: public fields added to a struct nobody builds by literal
  ship as a minor.
- **A requirement names the minor the code calls into** (`"1.2"`, not `"1"`),
  raised in the change that first uses the new API: a lockfile holding 1.0.0
  satisfies `"1"`. Path dependencies hide this inside the workspace; they
  still carry `version =`, which publishing keeps as the requirement.
- **Every published crate keeps a Keep a Changelog `CHANGELOG.md`.**
- **A crate whose API differs from its published version of the same number
  is bumped**: publishing resolves dependencies from crates.io only, so it
  breaks every dependent's publish.
- **Check concurrent version changes together.** Two pull requests green on
  their own can break `main` together; of two raising one crate to the same
  version, the second to merge takes the next one.
- **A crate stays unpublished until a consumer needs it** (the HAL crates).
  A new crate's first version is published by hand, because crates.io's
  trusted publishing cannot create a crate.
- **New crates and modules are `Apache-2.0`.** A crate linking GPL code stays
  GPL (`say-piper`).
- **A major of `arora-bridge` or `arora-types` is a two-repo release.**
  `arora`'s `studio-bridge` feature depends on `arora-studio-bridge-client`,
  which implements `arora_bridge::Bridge` from crates.io, so that feature
  fails to build until the client moves. The order: merge and publish here,
  then the client re-pins and publishes its major, then an `arora` patch
  re-pins the client. State it in the pull request. Do not gate pull requests
  on a Studio build: it is red by construction on the one carrying the major.

## Pull requests and issues

- **Issues live in Linear, not GitHub issues.** Link a pull request or ticket
  with a Markdown link: Linear rewrites a bare `#N` into a link to another
  repository's pull request.
- **A title states the change**, not the fault it fixes. The body opens with
  an **Intent** paragraph — what the change is for and what it deliberately
  does not do — then the current behaviour, the change, and why it matters.
- **Reviews, automated ones included, are measured against the intent.** A
  correction of what the pull request claims or does is applied and the
  thread answered with the fixing commit; a suggestion adding a capability
  beyond the intent is declined on the thread, and a real problem it names
  gets a ticket.
- **A finished pull request is ready, not draft**: draft reads as "not ready
  for review".
- **Pull requests are rebase-merged**, so each commit stands on its own and
  its message explains it.
- **A stacked pull request is retargeted when the one below merges.** Merged
  branches are not deleted, so GitHub does not retarget it, and merging it
  lands it on the dead branch. Retarget it to `main` and rebase it; after any
  merge, check `git merge-base --is-ancestor <commit> origin/main`.
- **Never bypass a hook or a check** (`--no-verify`). One failing on code you
  did not touch points at stale state — a dirty checkout, stale generated
  files — and that is what to fix.
- **Read `git diff --cached --stat` before committing.** A shared checkout can
  lose content under you, and `git add -A` then commits the loss as a revert;
  stage by path or with `git add -u`. Squash onto the branch's merge base,
  never with `git reset --soft origin/main`, which reverts whatever `main`
  gained since the branch was cut.

## Crates

| Crate | Purpose |
|---|---|
| `arora-types` | Values, types, calls, records, the data-store vocabulary |
| `arora-types-derive` | `#[derive(AroraType)]`: a Rust type as an Arora type |
| `arora-buffers` | The binary form of a value at module boundaries; serde to and from it |
| `arora-behavior` | The `BehaviorInterpreter` trait, the interpreter module's ids, task runs, built-in keys |
| `arora-hal` | The `Hal` trait, `FakeHal`, joint-target conventions, the HAL module's ids |
| `arora-bridge` | The `Bridge` trait, `FakeBridge`, the client conventions |
| `arora-simple-data-store` | `SimpleDataStore`, and `NamespacedStore` for many devices over one store |
| `arora-registry` | The local type registry and record files |
| `arora-registry-remote` | Registries backed by Semio's hosted store (unpublished) |
| `arora-engine` | Loads modules and dispatches calls: wasmtime components, the browser, native libraries, host modules |
| `arora-module` | Declare a module in Rust (the facade over `arora-module-macros`) |
| `arora-module-core` | Analysis and resolution of `module.yaml` declarations |
| `arora-module-cli` | The generator front end (unpublished) |
| `arora-module-rust`, `arora-module-cpp` | The Rust and C++ generators |
| `arora-behavior-tree` | The behavior-tree interpreter, its text language and Groot import |
| `arora-behavior-tree-types` | `Status` and `TickId` as module-facing types |
| `arora-behavior-tree-types-yaml` | The same types as YAML records |
| `arora` | The device runtime, its builder, and the `arora` runner |
| `arora-bridge-ws` | The open local bridge: JSON messages over a local WebSocket |
| `arora-bridge-ros2`, `arora-msgs-ros2` | The ROS 2 bridge; ROS 2 messages, their CDR codec and type hashes |
| `arora-hal-ros2`, `arora-hal-restful` | Robot HALs and their runners `arora-ros2`, `arora-restful` (unpublished) |
| `arora-web` | The device in a browser, for JavaScript |
| `arora-cli` | Load a module and call one of its functions from the command line (unpublished) |
| `arora-animation`, `arora-animation-module` | Animations and their exact evaluation; the module `@arora/animation` |
| `arora-geometry`, `arora-graphics-types` | Vectors, quaternions, transforms and colours over `Value` |
| `arora-dataflow-nodes` | Dataflow nodes as the functions of `@arora/dataflow`, `@arora/geometry`, `@arora/kinematics` |
| `arora-util`, `arora-vfs`, `wasi-sdk` | A C helper, an in-memory file tree for code generation, the WASI SDK for C++ guests |

## Related repositories

- [arora-aldebaran](https://github.com/semio-ai/arora-aldebaran) — NAO and
  Pepper support, against the published crates: the model for supporting a
  robot in a repository of its own.
- [semio-client](https://github.com/semio-ai/semio-client) — the client of
  Semio's hosted store, behind `arora-registry-remote`.
