# Building Arora

This page goes from a fresh checkout to every artifact the workspace produces:
what to install, the command that builds the default set and what that set
holds, how the pieces are wired, then the other targets — WebAssembly guests,
C++ modules, the browser runtime — and the features, environment variables and
fixes that change or rescue a build. It ends with how crates are released.
Running the tests is [`testing.md`](testing.md).

## What you need

| Tool | Why |
| --- | --- |
| [rustup](https://rustup.rs) | [`rust-toolchain.toml`](../rust-toolchain.toml) pins `nightly-2026-06-03` with `rustfmt`, `clippy`, `rust-src` and the `wasm32-wasip2` target. rustup installs them the first time cargo runs in the checkout; `rustup toolchain install` in the workspace root does it ahead of time. |
| A C and C++ compiler for the host | Native dependencies of the graph (`ring`, `zstd-sys`, …) compile C: the Xcode Command Line Tools on macOS, gcc or clang on Linux. |
| CMake 3.20 or later | The C++ modules build through CMake, and so does the audio library `polly` links. |
| Network access on the first build | crates.io; the git dependencies `semio-client`, `semio-record` and `semio-store-rpc`, public repositories under [github.com/semio-ai](https://github.com/semio-ai); and the WASI SDK, which the build downloads. |

Nothing else is needed to build and test the default set. The WASI SDK, which
compiles the C++ modules, is fetched by the build ([below](#the-wasi-sdk)). The
ROS 2 crates speak DDS and Zenoh through a Rust implementation, with no ROS
installation.

The toolchain is a nightly because the workspace uses one unstable cargo
feature, **artifact dependencies** (`bindeps`), which
[`.cargo/config.toml`](../.cargo/config.toml) turns on for the whole workspace.
An artifact dependency is a dependency on another crate's built output — a host
binary, a static library or a `cdylib` compiled for another target — rather than
on its Rust library. It is how a build script gets a code generator to run, or a
test gets a WebAssembly guest to load, without a nested `cargo` invocation.

The tools that only some targets need are listed with them: `wasm-pack` and
Firefox for [the browser runtime](#the-browser-runtime), `cargo-llvm-cov` for
[coverage](testing.md#coverage), Docker for one ignored ROS 2 test.

## Build

```bash
cargo build
```

[`Cargo.toml`](../Cargo.toml) lists every crate of the workspace under
`members`, and a subset under `default-members`. A cargo command run at the root
without `-p` or `--workspace` acts on the **default members**. `cargo build`
builds them for the host, in the `dev` profile, under `target/debug/`:

- **The binaries:** `arora`, the headless device runner
  ([`crates/arora`](../crates/arora/readme.md)); `arora-cli`, which loads
  modules and calls their functions
  ([`crates/arora-cli`](../crates/arora-cli/readme.md)); and the module code
  generators `arora-module-cli`, `arora-module-cpp` and `arora-module-rust`.
- **Every library crate:** the type vocabulary, the engine, the runtime and its
  HALs and bridges, behavior trees, the authoring crates.
- **The C headers** of the C ABI, which the build scripts of `arora-buffers`,
  `arora-util` and `arora-engine` write to `target/include/arora/`.
- **The Rust WebAssembly guests the integration tests load** —
  `test-rust-wasm`, `test-rust-component`, `test-behavior-tree-nodes` and the
  module `@arora/animation` (package `arora-animation-module`) — compiled for
  `wasm32-wasip2`, because the default member `arora-integration-tests` declares
  them as artifact dependencies of its build script.

`cargo build --release` builds the same set under `target/release/` with the
release profile `Cargo.toml` sets: optimised, thin LTO across crates
(`lto = "thin"`) and limited debug info (`debug = 1`).

### The members outside the default build

| Member | What it is | How it is built |
| --- | --- | --- |
| `polly` | A native module speaking through AWS Polly. Kept out so the default build skips the AWS SDK; it also builds the SoLoud audio library with CMake. | `cargo build -p polly` |
| `arora-web` | The browser runtime. It compiles for `wasm32-unknown-unknown`; on the host it is empty. | [The browser runtime](#the-browser-runtime) |
| `test-rust-wasm`, `test-rust-component`, `test-behavior-tree-nodes` | Rust test guests. | For `wasm32-wasip2`, by the crates that load them, as artifacts; `cargo build -p <name>` builds one for the host. |
| `test-cpp`, `test-cpp-2`, `example-cpp` | C++ guests. | `cargo test`, through the integration tests' dev-dependencies; `cargo build -p <name>` builds one. |

`cargo build --workspace` builds every member.

[`examples/policy-modules`](../examples/policy-modules/README.md) is a workspace
of its own, with its own toolchain file and lockfile. The root build does not
reach it; its README says how to build it.

## How the build is wired

### Artifact dependencies

The workspace declares three kinds:

| Kind | Declared as | Used for | Its path, in the consumer |
| --- | --- | --- | --- |
| A host binary | `artifact = "bin"` | `arora-module-cli` and `arora-module-cpp` in the C++ modules' build scripts; `arora-cli` in the integration tests' | `CARGO_BIN_FILE_<DEP>` |
| A static library for another target | `artifact = "staticlib", target = "wasm32-wasip2"` | `arora-buffers` and `arora-util`, linked into the C++ guests | `CARGO_STATICLIB_DIR_<DEP>`, `CARGO_STATICLIB_FILE_<DEP>_<lib>` |
| A `cdylib` for another target | `artifact = "cdylib", target = "wasm32-wasip2"` | The Rust guests that `arora-integration-tests`, `arora`, `arora-engine` and `arora-web` load in their tests | `CARGO_CDYLIB_FILE_<DEP>_<lib>` |

`<DEP>` is the dependency's name in upper case with dashes as underscores;
`<lib>` is its library's name, with dashes as underscores. The form without
`_<lib>` is set only when the artifact's name equals the dependency's: a binary
keeps its dashes, so `CARGO_BIN_FILE_ARORA_MODULE_CLI` is set, while the library
`arora_buffers` of the dependency `arora-buffers` gets no
`CARGO_STATICLIB_FILE_ARORA_BUFFERS`. Read the suffixed form
(`CARGO_STATICLIB_FILE_ARORA_BUFFERS_arora_buffers`) or the `DIR` form, as
[`modules/test-cpp/build.rs`](../modules/test-cpp/build.rs)
(`staticlib_artifact`) and [`tests/build.rs`](../tests/build.rs) do.

For a build dependency, these variables reach the build script alone:
`tests/build.rs` passes them on to the tests with `cargo::rustc-env`, and the
tests read them with `env!`. For a dependency or a dev-dependency, cargo sets
them while it compiles the crate, so its code reads them with `env!` directly.

### Rust guests

A Rust module crate (`modules/animation`, `modules/test-rust-wasm`) is a
`cdylib` and an `rlib` with no fixed target. Built on its own, it compiles for
the host; a consumer that needs the WebAssembly guest declares it with
`target = "wasm32-wasip2"`, and cargo builds it for that target as part of the
consumer's build. Built for `wasm32-wasip2`, `arora_module::artifact!` makes the
`cdylib` a component of the `arora:module` world
([`crates/arora-engine/wit/arora-module.wit`](../crates/arora-engine/wit/arora-module.wit)),
the one form a WebAssembly guest loads in. No step builds the guests with an
explicit `--target` for the tests.

### C++ guests

A C++ module is a cargo package whose Rust library is empty
(`modules/test-cpp`, `modules/test-cpp-2`, `examples/modules/cpp`). Its
`build.rs`:

1. locates the [WASI SDK](#the-wasi-sdk);
2. takes `arora-module-cli` and `arora-module-cpp` as binary artifacts and
   copies them into one directory, because `arora-module-cli` runs the
   generator for a language as `arora-module-<language>` beside its own
   executable;
3. takes `arora-buffers` and `arora-util` as `wasm32-wasip2` static libraries;
4. runs the module's `CMakeLists.txt` through the `cmake` crate, with the SDK's
   `wasi-sdk-p2.cmake` toolchain file and those paths as cache variables. It
   tells the `cmake` crate the target is `wasm32-wasip2` and to add no flags of
   its own (`.target`, `.host`, `.no_default_flags`), since the flags it would
   derive from the host target break the WASI toolchain.

The CMake project runs `arora-module-cli` on `module.yaml` to generate the C++
bindings, the module's header and its records; compiles them with the module's
sources and [`libs/cpp`](../libs/cpp/CMakeLists.txt) — `arora-cpp`, and
`arora-cpp-component`, which makes the guest a component of the `arora:module`
world; links the guest as a WASI reactor; and writes its **module directory**,
the form a device loads a module in:

```text
target/<profile>/modules/<name>/
├── header.json   the header arora-module-cli generated
├── <name>.wasm   the guest, a wasm32-wasip2 component
└── records/      the module's records
```

The generated C++ stays in the CMake build tree, under the package's
`OUT_DIR`. The build script runs again when `module.yaml`, `CMakeLists.txt`,
`src/`, [`libs/cpp`](../libs/cpp) or the WIT world change, so an edit to
`module.yaml` needs no `cargo clean`.

`test-cpp-2` calls a function of `test-cpp`: it depends on the `test-cpp`
package, so cargo builds `test-cpp` first, and hands `test-cpp`'s `records/` to
`arora-module-cli --include`, which resolves the import against them.

### The WASI SDK

The `wasi-sdk` crate ([`crates/wasi-sdk`](../crates/wasi-sdk/src/lib.rs)) gives
build scripts WASI SDK 33:

1. the directory `WASI_SDK_PATH` names, when it holds `bin/clang`;
2. otherwise `<target>/wasi-sdk-33/`, downloaded on first use from the
   [WASI SDK releases](https://github.com/WebAssembly/wasi-sdk/releases) for
   macOS, Linux or Windows on x86_64 or arm64. Build scripts running at once
   take turns on `<target>/wasi-sdk-33.lock`: one downloads, the others use
   its install.

`<target>` is `CARGO_TARGET_DIR` when it is set, the workspace's `target/`
otherwise. Only the C++ modules use the SDK, so `cargo build` never downloads
it and `cargo test` does.

### Files build scripts write outside `OUT_DIR`

| Build script | Writes |
| --- | --- |
| `arora-buffers`, `arora-util`, `arora-engine` (host builds) | `target/include/arora/buffers.h`, `util.h`, `arora.h`, which the C++ guests include |
| the C++ modules | their module directories, `target/<profile>/modules/<name>/` |
| `arora-behavior-tree-types-yaml` | the records of `Status` and `TickId` in its own `records/`, git-ignored |
| `test-behavior-tree-nodes` | the Rust bindings in its own `src/arora_generated/`, git-ignored, and its `records/`, committed, from its `module.yaml` |

These locations are fixed relative to the workspace: `CARGO_TARGET_DIR` does
not move them.

## Other targets

### A Rust guest on its own

```bash
cargo build -p test-rust-wasm --target wasm32-wasip2   # target/wasm32-wasip2/debug/test_rust_wasm.wasm
```

The animation module writes its module directory with an example program:

```bash
cargo build -p arora-animation-module --target wasm32-wasip2 --release
cargo run -p arora-animation-module --features module-dir --example module_dir -- \
    <dir> target/wasm32-wasip2/release/arora_animation_module.wasm
```

### A C++ module

```bash
cargo build -p example-cpp   # target/debug/modules/example-cpp/
```

[`examples/modules/cpp`](../examples/modules/cpp/readme.md) walks through the
module and loads it into a device. Its `CMakeLists.txt` lists the cache
variables to set to build it with CMake alone.

### The browser runtime

`arora-web` runs a device in a browser. It compiles `arora` and `arora-engine`
for `wasm32-unknown-unknown`: there the engine is the host, and it runs the
`wasm32-wasip2` guests through the browser's own `WebAssembly` runtime, so it
needs no WASI. `arora` and `arora-engine` build for that target only without
their default features, which are the native hosts; `arora-web` depends on them
that way.

```bash
rustup target add wasm32-unknown-unknown
cargo build -p arora-web --target wasm32-unknown-unknown
wasm-pack build crates/arora-web --target web --dev   # the JS package, in crates/arora-web/pkg/
```

`wasm-pack` installs with `cargo install wasm-pack`. `.cargo/config.toml` sets
`--cfg getrandom_backend="wasm_js"` for `wasm32-unknown-unknown`, which
`getrandom` needs to use the browser's WebCrypto; a crate built for the browser
outside this workspace sets it too. [`crates/arora-web`](../crates/arora-web/readme.md)
describes the JavaScript surface.

## Features

| Crate | Feature | Default | What it adds |
| --- | --- | --- | --- |
| `arora` | `native` | on | The wasmtime and native-library engine hosts, the local WebSocket bridge and the `arora` binary. Off for `wasm32`. |
| `arora` | `tui` | on | The terminal operator interface the binary uses on an interactive terminal. |
| `arora` | `studio-bridge` | off | The Semio Studio connection: `arora-studio-bridge-client` and the device's enrollment. A binary built with it connects to Studio unless its run passes `--no-studio`. |
| `arora-engine` | `wasmtime-host`, `native-host` | on | The component executor (wasmtime) and the native-library executor. Off for `wasm32`. |
| `arora-hal-ros2` | `dds`, `zenoh` | `dds` | The ROS 2 middleware; exactly one. |
| `arora-hal-ros2` | `runner` | off | The `arora-ros2` binary. |
| `arora-hal-ros2`, `arora-hal-restful` | `studio` | off | `arora`'s `studio-bridge`, for the runner. |
| `arora-hal-restful` | `runner` | off | The `arora-restful` binary. |
| `arora-bridge-ros2` | `dds`, `zenoh` | `dds` | The ROS 2 middleware; exactly one. |
| `arora-msgs-ros2` | `dds`, `zenoh` | off | REP-2016 type hashes over that middleware (`interop`). |
| `arora-animation-module` | `module-dir` | off | Writing the module's module directory. |
| `arora-dataflow-nodes` | `urdf_ik` | on | The URDF inverse and forward kinematics nodes. |
| `arora-types` | `derive` | on | `#[derive(AroraType)]`. |

```bash
cargo build -p arora --features studio-bridge                       # the runner, connecting to Studio
cargo build -p arora-hal-ros2 --features runner                     # target/debug/arora-ros2, over DDS
cargo build -p arora-hal-ros2 --no-default-features --features zenoh,runner
cargo build -p arora-hal-restful --features runner,studio           # target/debug/arora-restful, with Studio
```

Cargo unifies features across the packages one command builds: building both
runners with `studio` on one of them turns on `arora`'s `studio-bridge` for
both.

`arora-studio-bridge-client` comes from crates.io and depends on `arora-types`
and `arora-bridge` from crates.io. `[patch.crates-io]` in `Cargo.toml` points
both at this workspace's crates, so the graph holds one copy of each; two copies
would be two distinct `Bridge` traits that do not type-check together.

## Environment variables

| Variable | Read by | Effect |
| --- | --- | --- |
| `WASI_SDK_PATH` | `wasi-sdk` | Use the WASI SDK installed there instead of downloading one. |
| `CARGO_TARGET_DIR` | cargo, `wasi-sdk`, `tests/build.rs` | Moves cargo's output and the WASI SDK cache. The C headers and the C++ module directories stay under the workspace's `target/`. |
| `CARGO_PROFILE_RELEASE_LTO`, `CARGO_PROFILE_RELEASE_DEBUG` | cargo | Override the release profile; CI's test jobs set `false` and `line-tables-only` ([`testing.md`](testing.md#running-the-tests)). |
| `GECKODRIVER` | `wasm-pack test` | The Firefox driver for the browser tests ([`testing.md`](testing.md#browser-tests)). |

## Releasing

[`.github/workflows/release.yml`](../.github/workflows/release.yml) runs on every
push to `main`. It walks its `publishable` list in dependency order and
publishes each crate whose manifest version is not on crates.io yet, so a crate
is released by raising its version. It authenticates with crates.io's trusted
publishing (OIDC), with no stored token.

The published crates, in the order they are published:

`arora-types-derive`, `arora-types`, `arora-geometry`, `arora-graphics-types`,
`arora-buffers`, `arora-engine`, `arora-vfs`, `arora-behavior`,
`arora-module-macros`, `arora-module`, `arora-animation`,
`arora-animation-module`, `arora-msgs-ros2`, `arora-bridge`, `arora-bridge-ws`,
`arora-behavior-tree-types`, `arora-hal`, `arora-simple-data-store`,
`arora-registry`, `arora-module-core`, `arora-module-rust`,
`arora-behavior-tree`, `arora-dataflow-nodes`, `arora`, `arora-bridge-ros2`,
`arora-web`.

Every other member is never published. `arora-cli`, `arora-module-cli` and
`arora-registry-remote` depend on `semio-client` from git, which crates.io
refuses (`arora-registry-remote` also sets `publish = false`, as do
`arora-integration-tests` and `example-cpp`). The rest are outside the list:
`arora-hal-ros2`, `arora-hal-restful`, `arora-module-cpp`, `arora-util`,
`arora-behavior-tree-types-yaml`, `wasi-sdk`, `polly`, `say-silent` and the test
guests.

To release a crate:

1. Raise its `version`, following Semantic Versioning, and add the version's
   entry to its `CHANGELOG.md`.
2. Raise the requirement in the crates that use what the new version adds.
3. Merge to `main`. A crate that a crate of the same release depends on is
   published first, and the workflow retries a publish while the index catches
   up.

A crate that fails to publish leaves a warning in the run, and the run still
succeeds: read its warnings after a release. A new crate joins the list once it
exists on crates.io with a Trusted Publisher for `semio-ai/arora-sdk` and
`release.yml`; crates.io configures one only on an existing crate, so a new
crate's first version is published by hand.

## Troubleshooting

**`CARGO_BIN_FILE_ARORA_MODULE_CLI not set; bindeps may not be enabled (-Z bindeps)`.**
Cargo did not read `.cargo/config.toml`, or is not the pinned nightly. Build
from inside the workspace, without a toolchain override (`cargo +stable`,
`RUSTUP_TOOLCHAIN`).

**The WASI SDK download fails** (offline, behind a proxy, on another host).
Install WASI SDK 33 yourself and set `WASI_SDK_PATH` to it. A directory without
`bin/clang` is ignored, and the build downloads instead.

**`cargo fmt --check` reports an unresolved `mod arora_generated;`.** The
generated bindings of `test-behavior-tree-nodes` do not exist yet. Any build
that reaches it writes them: `cargo build`, or `cargo clippy --all-targets`.

**Two `Bridge` traits, or two `Value` types, that do not match.** A crate from
crates.io asks for a major of `arora-bridge` or `arora-types` that the
workspace does not carry, so cargo leaves `[patch.crates-io]` unused (it warns
that the patch "was not used in the crate graph") and builds a second copy.
Bring the requirement and the workspace crate's version to the same major.

**A build script's output.** Cargo shows what a build script prints only when
it fails. `cargo build -vv -p <package>` shows it on success; cargo also keeps it
in `target/<profile>/build/<package>-<hash>/output` and `stderr`.

**`getrandom` refuses to build for `wasm32-unknown-unknown`.** The
`getrandom_backend` cfg is missing: build from inside the workspace, or set it
as `.cargo/config.toml` does.
