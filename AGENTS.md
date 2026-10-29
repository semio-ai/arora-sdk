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
  protocol"; "slot" means `arora_types::data::Slot` only.
- **Semio Studio is opt-in.** Nothing in Arora connects to Studio by default:
  the connection sits behind the `studio-bridge` feature of `arora` and each
  runner's `studio` feature.
- Every relative link must resolve; CI link-checks every Markdown file.

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
