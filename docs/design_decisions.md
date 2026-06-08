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
  `CARGO_CDYLIB_FILE_BEHAVIOR_TREE_NODES_behavior_tree_nodes` or
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

`behavior-tree-nodes` and `test-rust-wasm` are plain
`crate-type = ["cdylib", "rlib"]` crates with **no** `forced-target` /
`package.target`. A bare `cargo build` compiles them for the host (so
`cargo test -p test-rust-wasm` runs natively). Their wasm32-wasip1 flavour is
produced on demand by whoever needs it:

- the integration-test crate declares them as
  `artifact = "cdylib", target = "wasm32-wasip1"` build-dependencies, so
  `cargo test` forces a wasm build and exposes each `.wasm` path; and
- CI additionally runs `cargo build -p <module> --target wasm32-wasip1
  --release` so the `arora-behavior-tree` unit tests can load the guests from
  `target/wasm32-wasip1/release/`.

(An earlier design used `forced-target` under `-Zper-package-target`; it was
dropped. The `cargo-features = ["per-package-target"]` lines still present in
`modules/test-cpp*/Cargo.toml` are vestigial and currently unused.)

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

### `wasm32-wasip1` for guest modules

Module guests target `wasm32-wasip1`, not `wasm32-unknown-unknown` or
`wasm32-wasip2`.

- `wasip1` is Tier 2 in rustc, fully supported by WASI SDK 33, and matches
  what wasi-sdk's clang emits by default.
- `wasip2` (component model) is a bigger ecosystem move; deferred.
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

### Three executors, one engine

The engine (`crates/arora`) exposes three `Executor` implementations and
selects between them at registration time:

| Executor                   | Module location               | Cfg                                  | Default feature |
| -------------------------- | ----------------------------- | ------------------------------------ | --------------- |
| `executor::native`         | `native.rs`                   | `cfg(not(target_arch = "wasm32"))`   | `native-host`   |
| `executor::wasm` (wasmtime)| `wasm/mod.rs`                 | `cfg(not(target_arch = "wasm32"))`   | `wasmtime-host` |
| `executor::browser`        | `browser/mod.rs`              | `cfg(target_arch = "wasm32")`        | always-on on wasm32 |

`native` uses `libloading` to dlopen host cdylibs. `wasm` uses `wasmtime`.
`browser` uses `js_sys::WebAssembly::{Module, Instance, Memory}` and
implements its own WASI stubs (`proc_exit`, `fd_write` → console,
`random_get` via `crypto.getRandomValues`, …).

`wasmtime`, `wasmtime-wasi`, `libloading`, and `tempfile` are gated behind
the `wasmtime-host` and `native-host` features (both default-on for native
builds). On `wasm32-*`, the defaults are off and the browser executor takes
over.

### `arora-web` is a separate crate

The wasm-bindgen JS surface lives in `crates/arora-web`, not inside
`crates/arora`. This mirrors the `vizij-rs/crates/animation/vizij-animation-wasm`
pattern: the core crate is dual-target with `cdylib, rlib`; the `-wasm`
crate is just the JS binding surface.

**Why:** keeps `wasm-bindgen` out of the dependency graph of native consumers
of `arora`, and lets `arora-web` be built/published independently.

### `engine as usize` for executor callbacks (deliberately unsafe)

Both the wasmtime and browser executors register `arora_dispatch` and
`arora_dispatch_indirect` host callbacks that capture an `*mut Engine`
re-encoded as `usize`, because `wasm-bindgen` `Closure`s and wasmtime's
`Linker` callbacks cannot capture borrowed lifetimes that span the engine's
lifetime cleanly. The engine is pinned (`Pin<Box<Engine>>`) so the address is
stable.

This is a known unsafe pattern; cleanup is tracked separately.

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

### Wasm guests for behavior-tree tests live at the workspace target dir

`crates/arora-behavior-tree/src/tests.rs` loads each module's `.wasm` from
`<workspace>/target/wasm32-wasip1/<profile>/<name>.wasm`. Profile is picked
via `cfg!(debug_assertions)`, so the path tracks `cargo test --release` /
`cargo test` automatically.

**Why:** the previous implementation looked in per-module `target/` dirs,
which were stale leftovers from the retired CMake build. Cargo-first builds
put all wasm artefacts in the workspace target dir.

**Caveat:** the `arora-behavior-tree` unit tests do not force a wasm build
themselves. An explicit `cargo build -p <module> --target wasm32-wasip1`
(or running the integration tests, whose bindeps force the guests) must
precede `cargo test`. CI does the explicit per-module wasm builds before
`cargo test --release`.

### Integration tests rely on a mix of bindeps and published artefacts

`tests/Cargo.toml` (`arora-integration-tests`) pulls in artefacts two ways:

- **Bindeps** (`[build-dependencies]`): `arora-cli` (`artifact = "bin"`),
  `behavior-tree-nodes` and `test-rust-wasm`
  (`artifact = "cdylib", target = "wasm32-wasip1"`), and `polly`
  (`artifact = "cdylib"`, host). `tests/build.rs` forwards their paths to the
  test binary via `cargo::rustc-env` (`ARORA_CLI_BIN`,
  `CARGO_CDYLIB_FILE_POLLY_polly`, `CARGO_CDYLIB_FILE_TEST_RUST_WASM_test_rust_wasm`),
  and `integration.rs` reads them with `env!`.
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
