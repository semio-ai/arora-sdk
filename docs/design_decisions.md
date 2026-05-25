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

Cargo exports `CARGO_BIN_FILE_<DEP>`, `CARGO_STATICLIB_FILE_<DEP>`, and
`CARGO_CDYLIB_FILE_<DEP>` to the consumer's `build.rs`.

**Why:** the stable alternative is a `build.rs` that shells out to a second
`cargo build` with environment variables scrubbed (`CARGO_BUILD_TARGET`,
`CARGO_TARGET_DIR`, `CARGO_ENCODED_RUSTFLAGS`, …) to avoid inheriting the
outer cross-compile. Bindeps removes that boilerplate and lets cargo do its
own caching properly.

**Trade-off:** requires nightly until cargo issue #9096 stabilises.

### Cross-target settings live in `.cargo/config.toml`

- `[unstable] bindeps = true` and `per-package-target = true` enable the
  unstable features used by the workspace.
- `[target.i686-unknown-linux-musl]` pins the Homebrew cross-compiler
  binaries on macOS (`brew install messense/macos-cross-toolchains/...`).

`cargo-features = ["bindeps"]` inside individual `Cargo.toml` files is **not**
sufficient: the canonical cargo spelling for these flags is in
`.cargo/config.toml`'s `[unstable]` block.

### `forced-target` for wasm-only modules

`behavior-tree-nodes` and `test-rust-wasm` are wasm-only by nature. They set
`forced-target = "wasm32-wasip1"` (under the `-Zper-package-target` unstable
flag), so `cargo build --workspace` builds them only for wasm32-wasip1 and
the host build never tries to link them as native cdylibs.

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

**Caveat:** the tests do not force a wasm build themselves. `cargo build
--workspace` followed by an explicit `cargo build -p <module> --target
wasm32-wasip1` (or running the integration tests, whose bindeps force the
guests) must precede `cargo test --all`. CI does this explicitly.

### Integration tests rely on workspace artefacts at known paths

`tests/Cargo.toml` (`arora-integration-tests`) bindeps the Rust wasm guests
(`behavior-tree-nodes`, `test-rust-wasm`) so they get built before the tests
run. Host-targeted modules (`polly`, `test-cpp`, `test-cpp-2`) are **not**
declared as bindeps — that triggers cargo's cdylib output-filename collision
warnings (cargo#6313). Those modules' `build.rs` scripts publish artefacts
to `target/<profile>/modules/`, which the integration tests read directly.

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
