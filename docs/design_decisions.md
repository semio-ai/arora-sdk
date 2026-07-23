# Design decisions

Decisions that shape the current state of the repo. Each entry records what
was chosen, why, and what alternatives were considered.

## Build orchestration

### Cargo, not CMake, is the top-level build driver

`cargo build --workspace` is the canonical build entry point. The root
`CMakeLists.txt` and `PreLoad.cmake` are gone. C++ modules still carry their
own `CMakeLists.txt`, but it is invoked by a Rust `build.rs` (via the `cmake`
crate) for IDE compatibility and standalone debugging.

**Why:** the previous CMake-first layout required users to pick targets via
CMake cache vars and re-run `cmake` for any cross build. Cargo-first lets
`--target`, `.cargo/config.toml`, and rust-toolchain.toml drive the build, and
puts cross-target artefacts in standard locations cargo's tooling already
knows about.

**Trade-off:** the workspace is pinned to nightly because we rely on
`-Z bindeps` (see below). That is acceptable for an in-tree build but rules
out shipping the workspace itself on crates.io without further work.

### `-Z bindeps` (artifact dependencies) over recursive cargo

Host code generators (`arora-module-cli`, `arora-module-cpp`) and cross-target
static libraries (`arora-buffers`, `arora-util` built for `wasm32-wasip1` or
`i686-unknown-linux-musl`) are pulled into consumer crates via cargo's
artifact dependencies:

```toml
[build-dependencies]
arora-module-cli = { path = "...", artifact = "bin" }
arora-buffers    = { path = "...", artifact = "staticlib", target = "wasm32-wasip1" }
```

Cargo exports the artifact paths to the consumer's `build.rs` as environment
variables. **Mind the exact names** — this has bitten us:

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
- `[target.i686-unknown-linux-musl]` pins the Homebrew cross-compiler
  binaries on macOS (`brew install messense/macos-cross-toolchains/...`).
- `[target.wasm32-unknown-unknown]` sets `getrandom_backend="wasm_js"` so the
  browser engine build (`arora-web`) selects getrandom's WebCrypto backend.

There is **no** `per-package-target = true` in the config: the workspace does
not actually use forced-target (see below).

### Wasm guests build for the host by default; wasm32-wasip1 on demand

`test-behavior-tree-nodes` and `test-rust-wasm` are plain
`crate-type = ["cdylib", "rlib"]` crates with **no** `forced-target` /
`package.target`. A bare `cargo build` compiles them for the host (so
`cargo test -p test-rust-wasm` runs natively). Their wasm32-wasip1 flavour is
produced on demand by whoever needs it: the integration-test crate declares
them as `artifact = "cdylib", target = "wasm32-wasip1"`, so `cargo test` builds
the wasm guests and the tests locate them through the forwarded
`CARGO_CDYLIB_FILE_*` env vars. No explicit `--target wasm32-wasip1` build is
needed — `cargo test --release` is self-sufficient.

(An earlier design used `forced-target` under `-Zper-package-target`; both it
and the leftover `cargo-features = ["per-package-target"]` opt-ins in the module
manifests have since been removed.)

### `cmake-rs` invoked with explicit target overrides

For cross-compiling C++ from a host cargo build script, cmake-rs reads the
build script's `TARGET` and synthesises flags like `--target=arm64-apple-macosx`
and `CMAKE_OSX_ARCHITECTURES=arm64`. Those are fatal for wasm and i686 cross
builds, so each C++ module's `build.rs` calls

```rust
cmake::Config::new(...)
  .target("wasm32-wasi") // or "i686-unknown-linux-musl"
  .host("wasm32-wasi")
  .no_default_flags(true)
  ...
```

The cmake "target triple" here is purely a hint to cmake-rs's flag synthesis;
the actual toolchain comes from `CMAKE_TOOLCHAIN_FILE` (wasi-sdk's, or the
musl cross-toolchain's).

## Target choices

### `wasm32-wasip1` for guest modules; `wasm32-wasip2` components incoming

Legacy module guests target `wasm32-wasip1`. New-style guests are
WebAssembly Components targeting `wasm32-wasip2` against the
`arora:module` WIT world (`wit/arora-module.wit`).

- `wasip1` is Tier 2 in rustc, fully supported by WASI SDK 33, and matches
  what wasi-sdk's clang emits by default. The custom malloc/dispatch ABI
  and `arora-buffers` exchange over raw linear memory live on this path.
- `wasip2` is Tier 2 in rustc and emits components directly (via
  `wasm-component-ld`). Components exchange data through the canonical
  ABI (`list<u8>`), eliminating the guest allocator protocol. See
  `modules/test-rust-component` and `executor::component`.
- `wasip3` (WASI 0.3, native async) is the destination: the WIT world is
  shaped so its functions become `async` without structural changes. Not
  adopted yet because wasmtime's `p3` module is explicitly experimental,
  `wasm32-wasip3` is Tier 3 in rustc, and browsers lack a transpilation
  path (jco async needs JSPI, absent from Firefox).
- `wasm32-unknown-unknown` is still used for pure-Rust wasm without a system
  interface (`arora-buffers`, `arora-util` as staticlibs, and the
  `arora-web` engine itself — see below).

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
scripts.

**Why:** older versions defaulted to `wasm32-wasi` (legacy alias); 33+
defaults to `wasm32-wasip1`, matching the Rust side. No suitable crates.io
crate exists (`lucet-wasi-sdk` is unmaintained since 2020).

### NAO is opt-in

The NAO module (`modules/nao`) cross-compiles to
`i686-unknown-linux-musl` and depends on a Homebrew cross-toolchain that is
not universally installed. It is excluded from `default-members` in the
workspace `Cargo.toml`. CI does not build it. Users with the cross-toolchain
can build it explicitly with `cargo build -p arora-nao`.

### libqi fetched via FetchContent

`modules/nao/CMakeLists.txt` fetches libqi from
`github.com/semio-ai/libqi.git` via CMake `FetchContent` (pinned commit).
This pulls Boost and OpenSSL transitively — expect ~10 min on a cold build.

## Engine architecture

### Four executors, one engine

The engine (`crates/arora-engine`) exposes four `Executor` implementations and
selects between them by the `executor.name` in a module's header:

| Executor                   | Name             | Module location  | Cfg                                | Default feature |
| -------------------------- | ---------------- | ---------------- | ---------------------------------- | --------------- |
| `executor::native`         | `native`         | `native.rs`      | `cfg(not(target_arch = "wasm32"))` | `native-host`   |
| `executor::wasm` (wasmtime)| `wasm`           | `wasm/mod.rs`    | `cfg(not(target_arch = "wasm32"))` | `wasmtime-host` |
| `executor::component`      | `wasm-component` | `component/mod.rs` | `cfg(not(target_arch = "wasm32"))` | `wasmtime-host` |
| `executor::browser`        | `wasm`           | `browser/mod.rs` | `cfg(target_arch = "wasm32")`      | always-on on wasm32 |

`native` uses `libloading` to dlopen host cdylibs. `wasm` uses `wasmtime`
core modules with the legacy malloc/dispatch ABI. `component` uses
`wasmtime::component` against the `arora:module` WIT world plus
`wasmtime-wasi` p2. `browser` uses
`js_sys::WebAssembly::{Module, Instance, Memory}` and implements its own
WASI stubs (`proc_exit`, `fd_write` → console, `random_get` via
`crypto.getRandomValues`, …).

`wasmtime`, `wasmtime-wasi`, `libloading`, and `tempfile` are gated behind
the `wasmtime-host` and `native-host` features (both default-on for native
builds). On `wasm32-*`, the defaults are off and the browser executor takes
over.

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
ownership so modules are callable without borrowing the engine — planned
together with the component-model migration.

## Module surface

### `module.yaml` is the single source of truth

Each module ships a `module.yaml` describing its types and functions.
`arora-module-cli` generates language bindings from it (`arora-module-rust`,
`arora-module-cpp`) and a "header" form with named symbols stripped that the
runtime uses for identification.

Module functions take and return a structure whose `id` matches the
function. The first field carries the return value; subsequent fields
correspond to mutated parameters. Values use `arora-types`'s externally-tagged
serde representation (`{f32: 0.5}`, not `{kind: "scalar", value: 0.5}`).

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
`load_behavior_tree_yaml` and ticks them directly. The crate's only
dev-dependencies are `anyhow` and `arora-simple-data-store` — there is no
wasm-guest bindep and no `CARGO_CDYLIB_FILE_*` env var forwarded from its
`build.rs`. (The `test-behavior-tree-nodes` wasm module still exists — it
carries wasm implementations of the same nodes — but it is test-only, and
nothing in the default build loads it for control flow.)

### Integration tests rely on a mix of bindeps and published artefacts

`tests/Cargo.toml` (`arora-integration-tests`) pulls in artefacts two ways:

- **Bindeps** (`[build-dependencies]`): `arora-cli` (`artifact = "bin"`),
  `test-rust-wasm` and `test-behavior-tree-nodes`
  (`artifact = "cdylib", target = "wasm32-wasip1"`), and `test-rust-component`
  (`artifact = "cdylib", target = "wasm32-wasip2"`). `tests/build.rs` forwards
  their paths to the test binary via `cargo::rustc-env` (`ARORA_CLI_BIN`,
  `CARGO_CDYLIB_FILE_TEST_RUST_WASM_test_rust_wasm`), and `integration.rs` reads
  them with `env!`. (`polly` is not a dependency of this crate — it is a
  workspace member staged under `target/<profile>/modules/`.)
- **Dev-dependencies** (plain path): `test-cpp` and `test-cpp-2`. They are not
  bindep'd — declaring an empty-lib C++ module as a `cdylib` artifact adds
  nothing and their `build.rs` is what matters. Listing them as
  dev-dependencies makes `cargo test` run those build scripts, which compile
  the wasm via cmake and publish `*.wasm`, `module.yaml`, and `records/` to
  `target/<profile>/modules/`. The C++ integration test reads those published
  files directly.

A bare `cargo build` (default-members) does **not** build `test-cpp`/`test-cpp-2`
(they are excluded from `default-members`); `cargo test` does, through the
dev-dependency edge. This is the asymmetry that makes the test step do work the
build step skips.

### CI builds release

`.github/workflows/continuous.yml` runs `cargo build --workspace --release`
and `cargo test --all --release` for both the host workspace and the
wasm32-wasip1 guest builds. The behavior-tree tests' profile selection picks
up `release` automatically. `arora-web`'s `build.rs` reads `PROFILE` so its
`include_bytes!` resolves to the release artefact when wasm-pack is invoked
with `--release`.

## Out of scope (recorded so we remember why)

- **Porting `arora-registry` and `arora-cli` to `wasm32`** — the browser
  engine accepts header YAML + bytes that the JS host fetched however it
  likes. Registry resolution and the remote-registry / auth flow stay
  host-only.
- **Replacing CMake inside individual C++ modules** — the orchestrator is
  cargo; what each module does internally to build its C++ is up to that
  module.
- **Reworking the module loader / VFS / runtime semantics** — the
  cargo-first work was a build-system change only.

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
`arora-registry-remote`, a private crate, because they depend on
`semio-client` — a private git dependency, and crates.io rejects git
dependencies even behind an off-by-default feature. A feature flag inside
`arora-registry` was the considered alternative; it reads nicer but is simply
not publishable. Both crates implement the same registry traits, so consumers
swap registries without code changes.

### Generated sources are committed

`arora-behavior-tree` ships its generated `src/arora_generated/` sources in
git and in the published crate. Its build script still regenerates them, but
the vfs `sync` is content-hash-guarded: an unchanged regeneration writes
nothing. Committed generated code keeps `cargo publish`'s tarball
verification happy (build scripts must not modify the source directory) and
means builds from the crates.io checkout never write into it either.

## Workspace topology

### One workspace, not a repo split

Everything lives in this single `arora-sdk` workspace: the engine, the
runtime, the vocabulary crates, the seams and their implementations, the
authoring tools, and the guest modules. A multi-repo split (separate
`arora-types`, `arora-ecbs`, `arora-sdk`, and a binary repo) was executed and
then reversed: with crates published to crates.io, external consumers already
get the narrow surface they need, and the split only added cross-repo
version choreography for every internal change. Publishing, not repo
boundaries, is the isolation mechanism.

## Runtime surface

### One builder, one step spine

`arora` exposes a single way to assemble a runtime: a builder over the four
seams (store, HAL, bridge, behavior), producing a synchronous `step(dt)` the
host paces — natively from `run`'s metronome, in the browser from
`requestAnimationFrame`. Each step publishes the built-in clock keys
(`arora/time`, `arora/dt`) to the store before the behavior ticks, so time is
data like everything else.

### The behavior interpreter is a module

Loading and editing behaviors go through the engine's ordinary module-call
path: `arora-behavior` declares the interpreter module's UUID and its
`load`/`edit` function ids, and the runtime builds that module
(`arora_engine::module::ModuleBuilder`) from the running interpreter. There is no
host-function special case in dispatch — a remote editing a behavior calls a
module function like any other, and interpreter implementations stay engine-
agnostic behind the `BehaviorInterpreter` trait.

### Predetermined keys are conventions, not wiring

Behaviors read their inputs from store paths and write outputs back; the
built-in keys are reserved names, not hard-wired node plumbing. Anything a node
would be "predetermined" to read can be overridden by linking a different
producer onto the same path.
