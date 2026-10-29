# Design decisions

Decisions that shape the current state of the repo. Each entry records what
was chosen, why, and what alternatives were considered.

## Build orchestration

### Cargo, not CMake, is the top-level build driver

`cargo build` is the build entry point: it builds the workspace's default
members, and `--workspace` adds the opt-in ones ([building](building.md)).
There is no top-level CMake project. Each C++ module carries its own
`CMakeLists.txt`, which its Rust `build.rs` invokes (via the `cmake` crate),
and which stays usable from an IDE or on its own for debugging.

**Why:** a CMake-first layout makes users pick targets through CMake cache
variables and re-run `cmake` for any cross build. Cargo-first lets
`--target`, `.cargo/config.toml` and `rust-toolchain.toml` drive the build, and
puts cross-target artefacts where cargo's tooling already looks.

**Trade-off:** the workspace is pinned to nightly because it relies on
`-Z bindeps` (see below). The published crates use artifact dependencies only
among their dev-dependencies, which publishing drops, so their consumers need
no nightly; the workspace's own build — guests, code generators, integration
tests — does.

### `-Z bindeps` (artifact dependencies) over recursive cargo

Host code generators (`arora-module-cli`, `arora-module-cpp`) and cross-target
static libraries (`arora-buffers`, `arora-util` built for `wasm32-wasip2`)
are pulled into consumer crates via cargo's
artifact dependencies:

```toml
[build-dependencies]
arora-module-cli = { path = "...", artifact = "bin" }
arora-buffers    = { path = "...", artifact = "staticlib", target = "wasm32-wasip2" }
```

Cargo exports the artifact paths to the consumer's `build.rs` as environment
variables. **Mind the exact names**:

- The fully-qualified form is `CARGO_<KIND>_FILE_<DEP>_<ARTIFACT-NAME>`, e.g.
  `CARGO_CDYLIB_FILE_TEST_RUST_WASM_test_rust_wasm` or
  `CARGO_STATICLIB_FILE_ARORA_BUFFERS_arora_buffers`.
- Cargo *also* emits the short convenience form `CARGO_<KIND>_FILE_<DEP>`
  **only when the artifact's target name equals the dependency name.** A
  `bin` target keeps its dashes (`arora-module-cli`), so the convenience
  `CARGO_BIN_FILE_ARORA_MODULE_CLI` is emitted. A Rust **lib** target
  normalises dashes to underscores (`arora-buffers` → lib `arora_buffers`),
  so the names differ and the convenience `CARGO_STATICLIB_FILE_ARORA_BUFFERS`
  is **never set** for dash-named staticlib/cdylib crates — only the
  `_arora_buffers`-suffixed form and `CARGO_STATICLIB_DIR_ARORA_BUFFERS` are.

Consumer `build.rs` scripts must read the suffixed form (or resolve via the
always-present `CARGO_<KIND>_DIR_<DEP>`); reading the bare convenience name for
a staticlib/cdylib silently yields "not set". See `modules/test-cpp/build.rs`
(`staticlib_artifact`) and `tests/build.rs` for the working pattern.

**Why:** the stable alternative is a `build.rs` that shells out to a second
`cargo build` with environment variables scrubbed (`CARGO_BUILD_TARGET`,
`CARGO_TARGET_DIR`, `CARGO_ENCODED_RUSTFLAGS`, …) to avoid inheriting the
outer cross-compile. Bindeps removes that boilerplate and lets cargo do its
own caching properly.

**Trade-off:** requires nightly until cargo issue #9096 stabilises.

### Cross-target settings live in `.cargo/config.toml`

- `[unstable] bindeps = true` enables artifact dependencies for the whole
  workspace. This is the canonical spelling; `cargo-features = ["bindeps"]`
  inside an individual `Cargo.toml` is **not** sufficient for bindeps.
- `[target.wasm32-unknown-unknown]` sets `getrandom_backend="wasm_js"` so the
  browser engine build (`arora-web`) selects getrandom's WebCrypto backend.

There is **no** `per-package-target = true` in the config: the workspace does
not actually use forced-target (see below).

### Wasm guests build for the host by default; wasm32-wasip2 on demand

`test-behavior-tree-nodes` and `test-rust-wasm` are plain
`crate-type = ["cdylib", "rlib"]` crates with **no** `forced-target` /
`package.target`. A bare `cargo build` compiles them for the host (so
`cargo test -p test-rust-wasm` runs natively). Their wasm32-wasip2 flavour, a
component, is produced on demand by whoever needs it: the integration-test
crate declares them as `artifact = "cdylib", target = "wasm32-wasip2"`, so
`cargo test` builds the wasm guests and the tests locate them through the
forwarded `CARGO_CDYLIB_FILE_*` env vars. No explicit `--target` build is
needed — `cargo test --release` is self-sufficient.

`forced-target` (under `-Zper-package-target`) would pin each guest to
wasm32 instead, at the cost of a second unstable cargo feature and of the
guests' host builds, which run their unit tests natively.

### `cmake-rs` invoked with explicit target overrides

For cross-compiling C++ from a host cargo build script, cmake-rs reads the
build script's `TARGET` and synthesises flags like `--target=arm64-apple-macosx`
and `CMAKE_OSX_ARCHITECTURES=arm64`. Those are fatal for wasm cross
builds, so each C++ module's `build.rs` calls

```rust
cmake::Config::new(...)
  .target("wasm32-wasip2")
  .host("wasm32-wasip2")
  .no_default_flags(true)
  ...
```

The cmake "target triple" here is purely a hint to cmake-rs's flag synthesis;
the actual toolchain comes from `CMAKE_TOOLCHAIN_FILE` (wasi-sdk's
`wasi-sdk-p2.cmake`).

## Target choices

### Guests are `wasm32-wasip2` components of the `arora:module` world

Every guest, Rust or C++, is a WebAssembly component of the `arora:module`
WIT world (`crates/arora-engine/wit/arora-module.wit`), built for
`wasm32-wasip2`, and its header names the `wasm-component` executor. One form
is what a module registry can hold and every host can run.

- The world is the contract: a component exports `functions` (the ids it
  serves, which the engine checks against the header at load) and
  `dispatch`, and imports what it calls — the world's `host` interface and
  the WASI 0.2 interfaces a host provides (`wasi:cli`, `wasi:io`,
  `wasi:clocks`, `wasi:random`, and `wasi:filesystem` with no directory
  preopened; no sockets), which
  `arora_engine::component_layout::HOST_INTERFACES` lists. The filesystem is
  there because a C or C++ guest's libc imports it as soon as it links its
  file descriptors' code — a debug build of the Rust libraries it links does —
  so refusing it would refuse guests that open no file; with no directory, a
  guest that does sees none, on either host. Data crosses as
  Arora Buffers in `list<u8>`, through the canonical ABI, so no guest
  allocator protocol is part of the contract. A guest's error is the
  `err` of `dispatch`; an error buffer in its `ok`, the form Arora Buffers
  give an error, is read as its error too.
- A component that traps is entered no more. wasmtime refuses to re-enter a
  component instance after a trap, since the trap may have left its state
  half-written, and the browser executor holds a trapped component to the
  same rule: every later call fails, naming the trap, until the module is
  reloaded, which instantiates it afresh.
- `wasip2` is Tier 2 in rustc, and WASI SDK 33 links components with
  `wasm-component-ld`. Rust guests export through `arora_module::artifact!`,
  C++ guests through `arora-cpp-component` (see the C++ section below).
- Both hosts run the same component. Natively wasmtime instantiates it; the
  browser instantiates its guest core module and gives it the canonical-ABI
  form of each import directly (`BrowserComponentExecutor`), so no
  transpiler or JS glue stands between the published component and a page.
  The browser does so only for a component whose own layer is what that
  stands for: imports from the interfaces every host provides, and the
  world's `functions` and `dispatch` with the world's types, each lifting
  the guest module's export of that name with its memory, `cabi_realloc`,
  UTF-8 strings and `cabi_post_` function, and a shim, fixups and initializer
  that do what `wit-component`'s do (`component_layout`). Anything else it
  reads is refused. It does not validate the rest of the binary, nor check how
  the guest module's own imports are wired, which the browser implements by
  name.
- `wasip3` (WASI 0.3, native async) is the destination: the WIT world is
  shaped so its functions become `async` without structural changes. Not
  adopted yet because wasmtime's `p3` module is explicitly experimental,
  `wasm32-wasip3` is Tier 3 in rustc, and browsers lack a transpilation
  path (jco async needs JSPI, absent from Firefox).
- A component is the one form a WebAssembly guest loads in. A
  `wasm32-wasip1` core module, whose header names `wasm`, has no executor: a
  second ABI to host on both runtimes doubles the surface every guest-facing
  rule (argument checks, limits, traps) must hold on, for guests that rebuild
  for `wasm32-wasip2` with no change to their source. A device refuses a
  module directory naming `wasm`, saying so.
- The world's package version, `arora:module@0.2.0`, is what a component is
  built against; a host provides that version's `host` interface, and a
  component built against another does not load.

### `wasm32-unknown-unknown` for the browser-hosted engine

The browser-hosted `arora` engine (`crates/arora-web`) targets
`wasm32-unknown-unknown` and uses the browser's native `WebAssembly` runtime
through `js-sys` / `web-sys`. It does not link wasmtime, libloading, or
tempfile.

**Why:** the engine inside the browser is the *host*, not a guest. It does
not need WASI; the browser is its environment. Targeting
`wasm32-unknown-unknown` keeps the surface small and avoids dragging in WASI
shims that would otherwise need to be polyfilled in JS.

### WASI SDK 33, fetched from Rust

`crates/wasi-sdk` downloads and caches wasi-sdk-33 to
`target/wasi-sdk-33/` on first use unless `WASI_SDK_PATH` is set. It exposes
`clang()`, `clangpp()`, and `cmake_toolchain_file()` to other `build.rs`
scripts. `cmake_toolchain_file()` is the SDK's `wasi-sdk-p2.cmake`, which
compiles for `wasm32-wasip2`, the target the Rust guests build for, and links
a component with `wasm-component-ld`.

**Why:** no suitable crates.io crate exists (`lucet-wasi-sdk` is
unmaintained since 2020).

### C++ guests are components and WASI reactors in module directories, as Rust guests are

A C++ guest is a component of the `arora:module` world. The sources
`arora-module-cli` generates from its `module.yaml` keep one entry point per
function and a table of them (`arora_module_exports`, `arora/module.hpp`);
`arora-cpp-component` (`libs/cpp/src/arora/component.cpp`), an object library
the guest links, serves that table through the world's `functions` and
`dispatch` exports and makes the generated code's calls to other modules
(`arora_dispatch`) through the `host` interface. Its link options name the
world for `wasm-component-ld` and export the `cabi_realloc` the canonical ABI
allocates with.

It links with `-mexec-model=reactor`: it has no `main` and no `_start`, and
exports `_initialize`, which the component's initializer — a module
`wit-component` adds — calls once when the component is instantiated, running
the C++ static constructors. A guest linked as a WASI command instead runs
them only from `_start`, which nothing calls, so a global with a dynamic
initializer stays zeroed memory.

Its build writes the module directory a device loads it from, the form a
Rust guest ships in: `header.json`, the guest as `<name>.wasm`, and
`records/` (`arora_module_directory` in `libs/cpp/CMakeLists.txt`). The
header `arora-module-cli` generates carries the module's version, so a
device describes the module from its record. `tests/tests/guest_parity.rs`
checks that the C++ and Rust guests have the same form: a component with the
world's exports, importing only what every host provides, around a guest
core module with the same target features.

### NAO through a Rust HAL, not a C++ module

The NAO is served by `arora-hal-naoqi`, a HAL over the pure-Rust
`libqi-vibe` crate, rather than by a C++ module linking libqi. There is no
C++ cross-build: the HAL builds for the host, and the cross-build of its
`arora-naoqi` runner to `i686-unknown-linux-musl` for the robot is opt-in and
not run in CI. It lives in
[arora-aldebaran](https://github.com/semio-ai/arora-aldebaran), with the
other Aldebaran robot support, and depends on the published Arora crates. See
[Deploy it on the robot](https://github.com/semio-ai/arora-aldebaran#deploy-it-on-the-robot).

## Engine architecture

### Three executors, one engine

The engine (`crates/arora-engine`) exposes three `Executor` implementations and
selects between them by the `executor.name` in a module's header:

| Executor                                     | Name             | Module location        | Cfg                                | Default feature |
| -------------------------------------------- | ---------------- | ---------------------- | ---------------------------------- | --------------- |
| `executor::native`                           | `native`         | `native.rs`            | `cfg(not(target_arch = "wasm32"))` | `native-host`   |
| `executor::component`                        | `wasm-component` | `component/mod.rs`     | `cfg(not(target_arch = "wasm32"))` | `wasmtime-host` |
| `executor::browser::BrowserComponentExecutor`| `wasm-component` | `browser/component.rs` | `cfg(target_arch = "wasm32")`      | always-on on wasm32 |

`native` uses `libloading` to dlopen host cdylibs. `component` uses
`wasmtime::component` against the `arora:module` WIT world plus
`wasmtime-wasi` p2, with wasmtime's concurrency support off: the world
declares no async function, stream or future, so a component that uses the
component model's concurrency ABI is refused at compilation.
`BrowserComponentExecutor` uses `js_sys::WebAssembly::{Module, Instance,
Memory}`: it instantiates a component's guest core module and implements, in
the canonical ABI's form, the `host` interface and the WASI 0.2 interfaces
the native executor links (standard output and error to the console, random
bytes from `crypto.getRandomValues`, clocks from `performance.now()` and
`Date.now()`, every pollable ready). Its `SharedLoader` stages a component
compiled asynchronously, for one above the browser's limit on synchronous
compilation.

`wasmtime`, `wasmtime-wasi`, `libloading`, and `tempfile` are gated behind
the `wasmtime-host` and `native-host` features (both default-on for native
builds). On `wasm32-*`, the defaults are off and the browser executor takes
over.

### A guest module compiles once; each engine instantiates it

Compiled code is shareable on both WebAssembly hosts, and only an instance
holds a guest's state: a `WebAssembly.Module` instantiates any number of
times, and a `wasmtime::component::Component` instantiates in any store of
the engine that compiled it. So `CompiledModule` holds the compiled code, and an engine's
`load_compiled_module` creates an instance of it; a device builder that loads
one into N devices compiles once, not N times. Loading a module from its bytes
is the same path with the compilation in front.

Natively, every `ComponentExecutor` in a process shares one wasmtime engine,
created on first use, rather than one each: a component compiled for one
engine cannot be instantiated in another, so per-executor engines would rule
out sharing. Each instance keeps its own `Store`, so guests share no state.
The engine allocates its instances on demand, reserving address space per live
instance. A guest's memory and the time of each call are bounded per store
(`arora_engine::limits`).

### `arora-web` is a separate crate

The wasm-bindgen JS surface lives in `crates/arora-web`, not inside
`crates/arora`. The engine crates (`arora-engine`, `arora`) are plain `rlib`s;
`arora-web` is the `cdylib, rlib` crate wasm-pack builds into the JS package —
the one place the binding surface lives.

**Why:** keeps `wasm-bindgen` out of the dependency graph of native consumers
of `arora`, and lets `arora-web` be built/published independently.

### Raw engine pointer in executor host state (deliberately unsafe)

All wasm executors give their dispatch callbacks access to a raw
`*mut Engine` (`EngineRef`): the wasmtime executors carry it in their
`Store` host state (behind an `EnginePtr` wrapper whose
`unsafe impl Send` satisfies wasmtime's store-data bound), the browser
executor captures it in its import closures. The engine is pinned
(`Pin<Box<Engine>>`) so the address is stable.

The pointer stays raw because dispatch is re-entrant: a guest call to
`arora_dispatch` re-enters `Engine::dispatch` while the engine is already
mutably borrowed further up the same stack, which no safe wrapper
(`RefCell`, `RwLock`) tolerates. Removing it means reworking module
ownership so that modules are callable without borrowing the engine.

## Module surface

### A module's interface has one source of truth, and which one depends on the language

A module written in Rust declares its interface in Rust, with
[`arora-module`](../crates/arora-module/readme.md)'s macros on the Rust module
and its functions: the crate that implements the module carries it. A module
written in another language ships a `module.yaml`, and `arora-module-cli`
generates the bindings from it (`arora-module-cpp`).

`module.yaml` is then one form the interface takes, not the interface: an
export writes it, from a declaration or from the authored YAML, in the
"header" form the runtime identifies a module by, every type name resolved to
its id. A store record is another form, and the two differ — records are versioned
and frozen, headers are not.

**The executor is the export's to name.** A declaration cannot know whether it
will be built native or wasm; the step that produces the artifact does, and a
header handed to `Engine::load_module` must carry it. A module linked into the
host has no header at all: it is registered from its exports and described by
its record.

Module functions take and return a structure whose `id` matches the
function. The first field carries the return value; subsequent fields
correspond to mutated parameters. Values use `arora-types`'s externally-tagged
serde representation (`{f32: 0.5}`, not `{kind: "scalar", value: 0.5}`).

### A contract's functions take `&mut self`

A contract (`#[arora_module::contract]`) declares functions that several
modules implement, each under its own module id. Its methods take
`&mut self`.

**The receiver names the implementation.** A trait is implemented for a
type, so every implementation has a type whether or not its methods take
`self`. The receiver adds a value of that type, which the host module owns
and hands to each call. An implementation with no state is a unit struct
(`struct Cloud;`). It is zero-sized: the value takes no memory, and nothing
is ever read through the reference. The receiver asks for nothing a trait
implementation does not already need.

**`&mut`, because the engine owns a host module exclusively.** A host
module's functions are `FnMut` closures, and dispatch takes the module by
`&mut`: one call at a time, with exclusive access. `&mut self` hands that
access to the implementation, which changes its fields without a lock.

**Considered: `&self`.** A stateful implementation would need interior
mutability (`RefCell`, `Mutex`) to change anything, for no gain: nothing
calls an implementation concurrently.

**Considered: `self` by value.** The first call would consume the
implementation.

**Considered: no receiver, dispatch by type.** The methods would be
`fn say(text: String) -> Status` and the host would call
`say::exports::<Cloud>()`, so a stateless implementation would carry no value
at all. A stateful one, though, could only keep its state in a process-wide
`static`:
- every module of that type in the process shares it, so two devices in one
  process share one set of runs;
- it needs a lock, because a `static` is shared across threads.

A `#[module]`'s free functions have the same limit, which is why a host whose
module needs state per instance cannot register it with `HostModule::of`.

One receiver form keeps the macro and its rules single. Receiver-less methods
could be added later without breaking an existing contract.

### Cross-language code-gen tools are co-located at runtime

`arora-module-cli` locates language-specific generators (`arora-module-cpp`,
…) as siblings of its own `argv[0]` directory. Bindeps put each binary in
its own dir, so consumer build scripts copy both into a single
`OUT_DIR/arora-tools/` before invoking `arora-module-cli`.

## Testing

### Behavior-tree tests run natively — no wasm guest

The basic control nodes are wired natively into `arora-behavior-tree`, so its
tests exercise the tree in-process with no wasm module at all:
`crates/arora-behavior-tree/src/tests.rs` builds trees with
`load_behavior_tree_yaml` and ticks them directly. Its dev-dependencies are
`anyhow`, `arora-simple-data-store` and `arora-dataflow-nodes` (for the text
language over a real function index); it has no wasm-guest artifact
dependency and no build script. The `test-behavior-tree-nodes` guest carries
wasm implementations of the same nodes; it is test-only, and nothing in the
default build loads it for control flow.

### Integration tests rely on a mix of bindeps and published artefacts

`tests/Cargo.toml` (`arora-integration-tests`) pulls in artefacts two ways:

- **Bindeps** (`[build-dependencies]`): `arora-cli` (`artifact = "bin"`), and
  `test-rust-wasm`, `test-behavior-tree-nodes`, `test-rust-component` and the
  animation module (`artifact = "cdylib", target = "wasm32-wasip2"`). `tests/build.rs` forwards
  their paths to the test binary via `cargo::rustc-env` (`ARORA_CLI_BIN`,
  `CARGO_CDYLIB_FILE_TEST_RUST_WASM_test_rust_wasm`), and `integration.rs` reads
  them with `env!`. `polly` is not a dependency of this crate.
- **Dev-dependencies** (plain path): `test-cpp`, `test-cpp-2` and
  `example-cpp`. They are not
  bindep'd — declaring an empty-lib C++ module as a `cdylib` artifact adds
  nothing and their `build.rs` is what matters. Listing them as
  dev-dependencies makes `cargo test` run those build scripts, which compile
  the guest via cmake and write its module directory — `header.json`,
  `<name>.wasm` and `records/` — to `target/<profile>/modules/<name>/`. The
  C++ integration tests read those directories directly.

A bare `cargo build` (default-members) does **not** build the C++ guests
(they are excluded from `default-members`); `cargo test` does, through the
dev-dependency edge. This is the asymmetry that makes the test step do work the
build step skips.

### CI builds release

CI tests the default members with `cargo test --release` (without cross-crate
LTO, see [the CI section of the architecture](architecture.md#ci)); the tests
that read the C++ guests from `target/<profile>/modules/` pick `release` from
`debug_assertions`. The browser tests take their guest from the artifact
dependency's `CARGO_CDYLIB_FILE_*` path, whatever the profile.

## Out of scope (recorded so we remember why)

- **Porting `arora-cli` and the remote registry to `wasm32`** — the browser
  device accepts headers, artifacts and records that the page fetched however
  it likes, and resolves them against a local registry. Remote resolution and
  its sign-in flow stay host-only.
- **Replacing CMake inside individual C++ modules** — the orchestrator is
  cargo; what each module does internally to build its C++ is up to that
  module.

## Records

### Type records live in arora-types

Type records (versioned declarations of structures, enumerations, modules and
folders — see [`docs/records.md`](records.md)) live in `arora_types::record`
rather than in a dedicated crate. The alternative — a separate `arora-record`
crate holding the record data types and the freeze machinery — would keep
`arora-types` smaller, and could still be extracted later without an API
change. We keep them together because declaring a type and pinning which
version of it you mean are one workflow: `arora-types` owns the factories that
produce structures from type specifications that can be versioned, and
splitting the specs from the versioning machinery would cut the factories off
from their inputs. Every consumer of records (registry, module authoring, the
behavior tree) already depends on `arora-types` anyway.

### Two type vocabularies, two axes

`arora-types` carries two encodings of "a type": `ty::{low,high}` and
`record::ty`. They overlap in shape but encode different axes. `ty::{low,high}`
is the **module-header** vocabulary — `high` uses string type ids as written in
`module.yaml`, `low` uses resolved UUIDs; the split is *parse/resolve*, and
these references carry no versions. `record::ty` is the **record** vocabulary —
`UnfrozenTy` carries version requirements, `FrozenTy` carries pinned versions;
the split is *version-pinning*, and the frozen form is a wire format. Collapsing
them into one parameterized vocabulary is tempting and remains an open
question; today the duplication is the price of keeping the module-header
format and the record wire format independently stable.

### The frozen serde shape is a wire contract

The Semio store consumes and serves the frozen record forms, so their serde
layout (adjacently tagged type expressions — `type`/`value` — camel-case
primitive kinds, `IndexMap` field order) is an external contract, not an
implementation detail. `arora-types` pins it with golden tests that
deserialize and round-trip verbatim copies of store-accepted YAML records
(`crates/arora-types/src/record/wire_tests.rs`). Change the serde attributes
only with a store-side migration plan.

### The remote registry is a separate, private crate

`arora-registry` is local-only and publishable. The registries that talk to
Semio's hosted store (`RemoteRegistry`, `RemoteCachedRegistry`) live in
`arora-registry-remote`, an unpublished crate, because they depend on
`semio-client` — a git dependency, and crates.io rejects git dependencies
even behind an off-by-default feature. A feature flag inside
`arora-registry` was the considered alternative; it reads nicer but is simply
not publishable. Both crates implement the same registry traits, so consumers
swap registries without code changes.

## Workspace topology

### One workspace, not a repo split

Everything lives in this single `arora-sdk` workspace: the engine, the
runtime, the vocabulary crates, the seams and their implementations, the
authoring tools, and the guest modules. A multi-repo split (separate
`arora-types`, `arora-ecbs`, `arora-sdk`, and a binary repo) adds cross-repo
version choreography to every internal change, while crates published to
crates.io already give external consumers the narrow surface they need.
Publishing, not repo boundaries, is the isolation mechanism.

Support for a particular robot is the exception, and lives in a repository of
its own that depends on the published crates: its HAL and runner change with
the robot, not with Arora, and its toolchain (a robot's cross-compiler, a
vendor SDK) stays out of Arora's build.
[arora-aldebaran](https://github.com/semio-ai/arora-aldebaran) carries NAO and
Pepper this way.

## Runtime surface

### One builder, one step spine

`arora` exposes a single way to assemble a runtime: a builder over the four
seams (store, HAL, bridge, behavior), producing a synchronous `step(dt)` the
host paces — natively from `run`'s metronome, in the browser from
`requestAnimationFrame`. Each step publishes the built-in clock keys
(`arora/time`, `arora/dt`) to the store before the behavior ticks, so time is
data like everything else.

The clock starts where the builder sets it (`with_start_time`, zero by
default), and only steps move it afterwards. A device that joins peers which
have already run takes their timeline with an ordinary first frame, because a
catch-up step would publish an `arora/dt` spanning the whole gap, and every
reader of `arora/dt` would see one frame that long. A running device has no
clock setter: a jump in `arora/time` would be the same gap seen from the other
key.

### A page's operations settle through a step at a zero dt

In the browser, a page's client operations (`call`, `invoke`, `spawn`, `halt`)
and its store writes, made while no `run()` is ongoing, are carried out by
`arora-web` asking for `Arora::step(Duration::ZERO)`. That step is taken in
the next microtask, and every request of one turn shares it. Nothing else
applies them outside a step. The step applies the inbound queue as every step
does (the page's operations in issue order, and the bridges' too), ticks the
behavior, and writes the HAL. So precedence, the meta checks, and change
notifications stay those of the one step spine, and a device nobody steps
still answers its page. A dedicated path that applied a call between steps
would leave the HAL unwritten and the behavior unaware until the next step.
It would also be a second order of writers to keep consistent with the step.
No time elapses in that step: what runs on time holds still, and only a
node that counts ticks advances.

While `run()` owns the device, its next step applies them, and the page adds
no step: the cadence is the run's. Read-only operations (`listKeys`,
`describeMethods`) never step. They read the store's sibling handle and the
method index (`list_keys`, `Arora::describe_methods`). A call from a callback
the step invokes cannot nest a step, so it is queued like any other. Its step
then follows as a task rather than a microtask, so a callback that asks on
every tick cannot starve the event loop. A behavior is where such work
belongs.

### A remote write is checked once, by the runtime

Every bridge's inbound `Update` reaches the device through the runtime's
command phase, so that is where it is checked against the store's meta, once
for every bridge: a key must be open (`editable`), and when its meta states a
type, the value must be of it (`Value::conforms_to`). A change with any refused
key writes nothing. The device's own writers — HAL, behavior, modules — are not
checked: the meta is the device's statement to the outside, not a constraint
it places on itself.

The type check is exact, with no conversion. A reader of a typed key matches
on the variant it holds; an `F32` in an `F64` key, or an `ArrayValue` in an
`ArrayF64` key, would fail that match as surely as a string would. Converting
on the way in would make the stored variant depend on the sender. A
`value::Type` names the outer shape only, so the contents of a compound value
(an option's content, a structure's record) are not checked.

### The behavior interpreter is a module

Loading and editing behaviors go through the engine's ordinary module-call
path: `arora-behavior` declares the interpreter module's UUID and its
`load`/`edit` function ids, and the runtime builds that module
(`arora_engine::module::ModuleBuilder`) from the running interpreter. There is no
host-function special case in dispatch — a remote editing a behavior calls a
module function like any other, and interpreter implementations stay engine-
agnostic behind the `BehaviorInterpreter` trait.

**The interpreter module's task-run functions are described.** `spawn`,
`spawn_graph` and `halt` are in the device's method index like any module's
functions, so a client finds them by name over `DescribeMethods` rather than
from `arora-behavior`'s ids. A call, a graph and a task handle travel as
key-values keyed by field name, so they are described as the dynamic
key-value type. `load` and `edit` act on the main behavior and are reached by
id, leaving their names to the typed functions of a composed behavior host.
A tree does not call these functions: they act on the interpreter that ticks
it.

**The interpreter describes the task runs it implements.** A method whose run
is behavior the interpreter hosts (a node-graph fragment) has no module of
its own. The interpreter lists it (`described_methods`), and the runtime
indexes it under the interpreter module, the module a remote spawns task runs
through. The alternative, a host module per such method whose function only
fails, would describe a method under a module that does not implement it;
every device would repeat it.

**A run's program can be a graph.** `spawn_graph` grafts a `Graph` as a task
run beside the main behavior; `edit` reaches its nodes and `halt` drops it. A
client that gives a device a program of its own — a behavior that calls a
module each tick and writes what it returns — therefore neither evicts the
behavior the device runs, as `load` would, nor needs a module of its own on
the device. A change to a running program is one `edit` of its links, applied
between two ticks, and so is a new program for the run: a `spawn_graph` of a
graph with its root.

**A change restarts only the nodes it touches.** The interpreter lowers the
whole changed graph again, and the new tree takes over the state of every node
the change left in place, by node id: a `SequenceStar`'s position, a run's
latch, a tree-local variable such a node reads. A node the change adds,
replaces, relinks or re-keys starts afresh, and a tree-local variable does
when every node reading it does: a load keeps one a run also reads, as the run
the load leaves in place sees it. Spawning, editing, halting or ending one run
therefore leaves the main behavior and every other run where they stand.
Lowering each run on its own would confine a change too, but runs and the
main behavior can share what a tree holds: an edit may place one node under
both, or link a run's node to a port of the main behavior's, and both may read
one tree-local variable. Separate lowerings would each hold their own copy.

**A graph run is known by its root.** The client chooses a graph's node ids,
so the graph's root names its run. Spawning a graph whose root a live run has
starts no second run: the run takes the graph as its program, only what
differs changing and the nodes left as they are keeping their state, and the
spawn answers the run's handle. A client that lost a run's `TaskId` (a page
reload, a second tab) takes the run over with the spawn it would make anyway,
a retried spawn starts no second copy, and a new program replaces the old one
between two ticks.

The run's handle comes back as the spawn's answer because an answer is the
one thing that reaches every client as a value: a bridge carries a refused
call's reason as text, and a `BehaviorError` is a message. The alternatives
each keep a refusal a client would have to read. A `halt` by root adds a
function to the interpreter module and leaves a new program to two calls, a
halt and a spawn, which a remote may deliver on different steps, leaving a
step with no program or with two. A replace policy would sit in `RunPolicy`,
which `spawn` shares although a spawned call has no root its client chose, and
which is reserved for arbitration between runs.

**A graph travels with its type and version.** `spawn_graph` takes the
graph's language (`behavior-tree`) and the version of that language's format
as arguments beside the graph, and the interpreter refuses a type it does not
read. A minor version adds to a format, so an interpreter reading `1.1` runs a
`1.0` graph; a major changes what a graph means. The type is an argument, not
a field of `Graph`, so a host can route a call on it without decoding the
graph.

### The HAL is a module

The device's HAL answers remotes through the same module-call path as the
interpreter: `arora-hal` declares the HAL module's UUID and its function ids
(`hal_module`), and the runtime builds that module from the HAL it owns. Its
functions serve the HAL's components' models — `models()`, `model_glb(component)`
and `model_glbs()` — as [the HAL components design](proposal-hal-components.md)
sets out: a model travels as a published reference a client resolves with its
own rights, and the device serves bytes only for a model its HAL states
servable, which the module checks before it asks the HAL for them. A
bridge-side special case would answer a call no module declares, and
`DescribeMethods` would not list it.

A HAL reaches the runtime as `Box<dyn Hal>`, so the models are an accessor on
`Hal` (`assets`, `None` by default) rather than a second trait the runtime
cannot query on a trait object. A device's models are fixed for its life, so
`HalAssets` answers synchronously, from what the HAL was built with, and the
module's functions answer from the step.

### A HAL describes its keys, and the store holds the description

A HAL states which keys it reads from a write (its inputs: actuator targets)
and which it reports (its outputs: sensed values), each with its `KeyMeta`:
the type it takes, and the unit, range and rest value where it knows them.
Inputs are `editable`; outputs are not. The runtime states that description to
the device's store with `DataStore::set_meta` when the device is built, so the
store remains the one place a key's meta is asked of: the runtime checks a
remote write against it, and every bridge lists it (`ListKeys`) with nothing
of its own to declare. A device's inputs are open because its HAL says so.

The description is a feed (`Hal::describe_keys`), not a query, because a HAL
can learn its keys after it starts: topics discovered on a ROS 2 graph, a
configuration fetched from the robot. Its first item describes what the HAL
knows when the device is built; each later item is stated at the next step,
before that step applies any write. An item adds or replaces, as `set_meta`
does; nothing retracts a key, because the store has no statement that does
either. A HAL whose keys are fixed yields one item.

The HAL and the store therefore share the description's vocabulary — a map of
keys to `KeyMeta`, the argument of `set_meta` — rather than one trait with the
store's `meta` and `all_meta`. A HAL whose keys appear while it runs has to
tell the device when they do, which a query cannot; and moving `meta` and
`all_meta` out of `DataStore` would be a major of `arora-types` that changes
no behaviour.

A key's own meta is more specific than a prefix's, so a key the HAL describes
takes the HAL's meta, whatever a prefix says: under `arora --open` (the empty
prefix opened) or a subtree a host opened, the HAL's outputs are closed and its
inputs take only their declared type. Every key the HAL does not describe keeps
what the store says of it, so an undescribed key under an opened prefix stays
open — except the runtime's own: the build states the `arora` subtree closed,
because a sandbox flag that opens everything must not hand remotes the
device's clock and its task runs' state, and a host that means to open one of
those keys states that key's own meta.

A key's own meta that the host states is the host's. The runtime fills, from
the HAL's description, the keys that have no meta of their own, and replaces
only the meta it stated from an earlier description; so a host narrows or
closes one of its HAL's inputs by stating that key's meta, before the build or
after it. The host is the authority on what its device opens; the HAL only says
what its hardware takes.

### A device carries its modules as module directories

Every device runner — `arora`, and the robot runners `arora-ros2`,
`arora-restful` and arora-aldebaran's `arora-naoqi` — loads the guest
modules under its device directory's `modules/` (and any `--module` directory): each a `header.json`
beside its artifact, and optionally the module's records in `records/`. An
operator installs a module by copying a directory, with no custom binary; the
embedder path (`AroraBuilder::with_module`, `with_registry`) stays what it is,
and the runners feed it through one builder call,
`AroraBuilder::with_module_dirs`, so a robot carries a guest module — the
animation module, a policy — exactly as the plain runner does. A runner that
loaded no module directory would make a module installed on the device
present on one runner and absent on another, with nothing at start to say so.

The module directories' records are kept apart from the registry an embedder
gives (`with_registry`) and join it when the device is built, refusing a
record that differs from one it holds. Merging at build makes the order of the
two calls irrelevant: a registry given after the directories would otherwise
replace their records, and the record-typed exports would fall back to their
headers, undescribed, with no more than a warning. A directory is read when
it is given, so a directory that does not read fails a runner's start before
the robot is reached; what the engine and the registry decide waits for the
build.

The artifact is found by its extension — the one `.wasm` for the `wasm`
executor, the one dynamic library for `native` — rather than by a fixed name
or a path in the header. The header carries no path (it is written at export,
before anyone decides where the artifact lands), and a fixed name would make
every published artifact directory need a rename; the extension is already
determined by the executor. One artifact per directory keeps the choice
unambiguous.

A directory that does not load fails the start, naming the module. The device
was told to carry it: starting without it would serve a device whose
`DescribeMethods` and dispatch disagree with what the operator installed,
which is harder to notice than a start that says what is wrong. Two
exceptions are deliberate: an entry whose name starts with `.` is OS metadata
(`.DS_Store`, an AppleDouble `._x.wasm` beside its `x.wasm`) — a device
directory copied through a file manager would otherwise never start — and a
`modules/` without any directory carries no module.

One module id, one module, refused by the builder: every source of modules
(an embedder's `with_module` and `with_host_module`, the device directory,
`--module`, the HAL module and the interpreter module) converges in
`AroraBuilder::build`. The engine answers a guest under an already-loaded id
with Ok, and registering a host module under a taken id replaces the module
there, so either case would leave a device that describes one module's
functions and dispatches another's. A host module has no name, so the error
names it by the functions it describes. The device directory reader refuses
two directories carrying one module earlier, naming both directories, because
the builder sees headers, not where they came from.

### Keys are paths; attributes are a convention

A key (`arora_types::data::Key`) is a path of `/`-separated segments, and that
is all of its structure. A trailing `.attribute` on the last segment
(`head_yaw.target_position`) is a naming convention that means something only
to the consumers that adopt it: the HALs name a joint's setpoint and sensed
value that way, and a behavior graph's `Select` writes the path into a value as
attributes. Stores, bridges, subscriptions and key meta give `.` no meaning; it
is one more character of the name. A consumer that adopts the convention states
it as its own naming contract, as each HAL does.

Most keys carry no attribute: the runtime's clock (`arora/time`, `arora/dt`),
the exposure profiles' keys (`standard/ros4hri/expression/…`), Vizij's face
(`display/face/compressed`), an animation instance's `animation/<instance>/<k>`.
And the layers where keys meet the outside — transports, ACLs, subscriptions —
work on paths. Components read by every layer would fit the HALs' joints, make
every other key a degenerate entity, and give each layer that translates them a
second spelling of one name: a transport that maps the attribute separator onto
its own path separator makes `arora/time` and `arora.time` one name on the
wire, so a grant or a subscription on either covers the other.

So:

- A bridge carries a key as its path (`arora-bridge-ros2`'s default topic is
  `/{namespace}/keys/{path}`) and translates no `.`; a key exposed under
  another name gets it from configuration (an exposure profile), not from its
  attributes. A key meant to travel keeps to the characters its transports
  carry (a ROS 2 topic name has no `.`).
- Prefix meta and prefix subscriptions cover subtrees on `/` boundaries
  (`prefix_covers`): `face` covers `face/mouth`, and `robot/joint1` does not
  cover `robot/joint1.position` — an attribute is part of the last segment's
  name, not a level below it.
- `Key`'s attribute helpers (`get_entity`, `get_attributes`, `get_component`,
  `with_component`, `from_parts`) serve the consumers of the convention. Code
  that does not adopt it ignores them, and a key it names needs no attribute.

A proposal, [target keys](proposal-target-keys.md), names a target as an
entity under the entity it targets — `hip_yaw/target.position` beside the
measured `hip_yaw.position` — in place of `hip_yaw.target_position`, with both
forms accepted while clients move.

### Predetermined keys are conventions, not wiring

Behaviors read their inputs from store paths and write outputs back; the
built-in keys are reserved names, not hard-wired node plumbing. Anything a node
would be "predetermined" to read can be overridden by linking a different
producer onto the same path.

### Semio Studio is opt-in

Arora is free software, and Studio, its bridge and its backend are a
commercial service beside it: free software does not sign its users up to a
commercial service, nor report to one, unless they opted in. The opt-in is the
build. The connection sits behind arora's `studio-bridge` feature, and each
robot runner's own `studio` feature, which no default build enables — the
runners' `runner` feature included. A runner built with it is Semio's runner
build, made to run with Studio, so its runs connect by default, on a terminal
or headless; `--no-studio` opts one run out and serves the open local bridge.
A build without it has no Studio option at all. A second, run-time opt-in is
not kept: it would make every run of a build that exists for Studio repeat the
choice its build already made, and a deployment that forgets the flag would
silently run without Studio.

What a run does is decided by the runner built with Studio, not by the
feature: a feature of arora is enabled for every crate a build links, so a
runner built without Studio beside one built with it sees `studio-bridge` on.
An `AroraBuilder` that no runner hands a choice therefore serves the open local
bridge, and a runner flattens `--no-studio` only under its own `studio`
feature. An embedder connects by calling `studio::connect`, or by handing
`Choice::Studio` to `AroraBuilder::with_studio`. An app whose installs nobody
set up must contact nobody keeps the device's `StudioDevice` instead
(`studio::of`), the device client's object for the device's Studio side: it
connects the device only when it holds a credential, and enrolls it when a
person asks.

### A device enrolls with Semio Studio

A device joins Studio by enrolling: a person approves it in Studio and chooses
its owner, and Studio creates the device and gives it a credential marking it
as a device. The device never declares its owner nor writes its own record: an
owner a device names itself is one nobody accepted, and a record the device
writes is one Studio cannot trust. So arora neither asks for an owner nor
reads one from the environment, and after enrollment the device's name and
owner are Studio's. The device writes only its model, which it knows better
than anyone: each run reports the fields the environment and the HAL give,
and only those, so a family the operator typed at enrollment, for a device
nothing else describes, stays the record's.

A run that connects to Studio and finds the device not enrolled enrolls it
then, without a separate command: building with Studio is the opt-in, and a
person's approval in Studio is the consent the enrollment needs. Enrolling as
a command of its own is Semio's client's, `semio device enroll NAME`, which
saves the same credential under the device's name: arora's own verbs never
reach Studio, and only a build with `studio-bridge`, Semio's runner build, reads
that credential. Without an enrollment token, it enrolls by code, headless
included: the code is in the log — or on standard error when the log shows no
info lines, since the run waits on someone seeing it — and the run waits for a
person's decision, so a service started from a build with Studio joins Studio
once someone approves it, and never before. A refused code, or a refused
token, fails the run instead of serving the local bridge: the run is one of a
build made for Studio, and serving something else would hide that Studio
refused the device.

Only a device the client finds not enrolled enrolls. Studio failing otherwise
says nothing of the enrollment, and a credential of another Firebase project
than the run's says the configuration and the device disagree on which Studio
the device belongs to: either way enrolling would replace a credential that
may be good, so the run fails and says what to do.

The enrollment token is read from the environment only. A command-line option
would put a secret where the host's other users read it (`ps`) and where the
shell keeps it (its history); the environment of a service is its own. arora
never logs it.

### A device's name keys its Studio credential

A run is a device named on its machine (`--device`, `ARORA_DEVICE`, `default`
when unset), and the name is the only key between arora and Semio's
enrollment: Semio's device store (`semio-studio-auth`) maps a name to its
credential, in a directory of its own apart from the device directory. arora
names the device and decides when a run enrolls; where a Studio credential
lives is Studio's. Keeping the credential in the device directory would make
every Semio tool that enrolls a device (`semio device enroll`) compute arora's
directory layout, and a build without Studio would carry a Studio
subdirectory it never reads.

The device directory stays arora's, for what the device keeps of its own
(its modules), and `--device-dir` places it anywhere. A run that connects to
Studio given a directory and no name is refused, naming the fix: unnamed, two
such devices would share the credential of `default`, each one's enrollment
replacing the other's. Without Studio a directory alone is fine, since
nothing is keyed by the name.

The deprecated `DEVICE_DIR` given alone keeps its credential in
`<DEVICE_DIR>/studio/`, where an earlier arora kept it, with no store and no
move. It is the variable deployments that run several devices already set,
one directory each: moving their credentials into the store under the one
name they share would merge them. Every other credential found in a device
directory's `studio/` moves into the store under the device's name once, so
an upgrade keeps the device enrolled.

arora validates the name itself, with the same rule as the store's
`DeviceName` (exactly one normal path component, so not `D:bench`, which
Windows joins as a path on another drive): a build without Studio links no
Semio crate.

The local bridge's port is an option of the device too
(`--bridge-port`, `ARORA_BRIDGE_PORT`, 9000 when unset): two devices on one
host cannot serve one port.
