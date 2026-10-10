# arora

The **opinionated Arora runtime**. Where [`arora-engine`](../arora-engine) is
the bare, unopinionated engine, this crate wires the whole device into one
object, [`Arora`]: the engine (with the WebAssembly and native executors), the
shared data store, the HAL and bridge I/O seams, one behavior interpreter, and
the step loop that drives them. The basic behavior-tree control nodes are
native in [`arora-behavior-tree`](../arora-behavior-tree), so no module needs
to be loaded to run a tree of them.

> **Architecture, with diagrams:** [`docs/runtime-and-data-flow.md`](docs/runtime-and-data-flow.md)
> shows all of the device's components and how data flows between them, plus a
> sequence diagram of one `step`. It pairs with
> [arora-behavior's interpreter workflow](../arora-behavior/docs/interpreter-workflow.md).

## Build and run it

The crate is a default workspace member, so `cargo build` produces the `arora`
binary: the headless device runner — no HAL-attached display; a head like Vizij
embeds the library instead. It reads its configuration from the environment,
serves the open local bridge on `ws://127.0.0.1:9000` in the default build,
and connects to Semio Studio in a build with the `studio-bridge` feature. The
front end is the terminal operator UI when the process is attached to a
terminal (feature `tui`), headless otherwise.

```sh
cargo build                      # builds arora into target/debug/arora

# install a Groot behavior tree as the device's behavior, then serve
./target/debug/arora crates/arora/examples/hello_tree.groot.xml

# or just serve, waiting for a behavior over the bridge
./target/debug/arora

# load a module from a directory besides those in the device directory
./target/debug/arora --module node_modules/@vizij/animation-module/artifact
```

At start the device loads every module in its device directory's `modules/`
(the layout is [below](#where-a-device-keeps-its-data)), then each `--module`
directory. A module that cannot be loaded fails the start, naming it.

## Use it as a library

Build a device with the [builder](src/lib.rs): pick a data store, a HAL, zero
or more bridges, the modules whose functions behaviors may call, and the
behavior interpreter — each with a default, so `Arora::builder().build()` is a
self-contained in-process device (fake HAL, no bridge, an empty behavior, a
private `SimpleDataStore`). Drive it with `step` once per frame, or `run`, the
visible loop over `step`. The run family at the crate root is sugar over the
builder:

```rust,no_run
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // The default device: in-process fake HAL, the open local bridge.
    arora::run().await
}
```

```rust,no_run
// A device over your hardware — the one call that turns a HAL into a running
// device. `run_with` also takes the bridge and the store; `run_with_frontend`
// the operator front end as well.
arora::run_with_hal(Box::new(my_hal)).await?;
```

Stopping a run is dropping its future: everything the run holds lives inside
it, so dropping it is a complete teardown, after which a host can build and run
a fresh device.

A device-specific Arora is built from the outside, with no fork and no
per-device feature flag: the [`device` example](examples/device.rs) provides
its own `Hal` and hands it to `run_with`; swap in a real robot HAL and the
studio-bridge connector and it is a real device build. `DeviceCli` is the clap
options the binary parses (the Groot file, `--open`, `--module`); flatten it
into a larger CLI and inject the results through the builder —
`DeviceCli::modules` reads the modules it names together with the device
directory's, each for `with_module`.

On wasm, build the library with `--no-default-features`: the `native` feature
carries the wasmtime and dynamic-library hosts, the operator flow, and the
binary.

## Where a device keeps its data

A device keeps what is its own from one run to the next in its **device
directory**, each use in a subdirectory of its own:

```text
<data_local_dir>/semio/arora/      per user: ~/Library/Application Support (macOS),
│                                  ~/.local/share (Linux), %LOCALAPPDATA% (Windows)
└── devices/
    └── <local id>/                the device directory: DEVICE_LOCAL_ID, `default` when unset;
        │                          DEVICE_DIR replaces this whole path
        ├── modules/               the modules the device loads at start, in name order
        │   └── <name>/            a module directory (what --module names elsewhere):
        │       ├── header.json    the module's header (its low-level Header, as JSON)
        │       └── *.wasm         its artifact: the one file with the executor's extension —
        │                          .wasm for wasm, .so / .dylib / .dll for native
        └── studio/                the Studio connection's credentials, kept by
            ├── key                arora-studio-bridge-client's DeviceCredentials
            └── refresh_token
```

The local id tells apart the devices one user runs on one host. It is local
only: the name a device shows (`DEVICE_NAME`) can change without making it
another device, and the id Studio knows it by is assigned at its first sign-in.
Anything else a device keeps goes in a subdirectory of its own beside these.

A module directory is the layout a module is published in: the
`@vizij/animation-module` package's `artifact/` — `header.json` beside
`vizij_animation_module.wasm` — copied to `modules/animation/` is loaded at the
next start. The artifact's name is free; its extension names it, so a module
directory holds one artifact. Entries whose name starts with `.` are neither
module directories nor artifacts: OS metadata (`.DS_Store`, an AppleDouble
`._x.wasm`) lands beside what it describes and is ignored. A module the device
cannot load — a header it cannot read, no artifact or several, an executor it
does not run, one module in two directories, an artifact the engine rejects —
fails the start, naming the module. A loaded module's functions are reachable
by any call — in-process (`Arora::call`), over a bridge — and `DescribeMethods`
lists them under one rule: a function whose parameters and return are all
primitives or optionals over a scalar primitive is described; any other — one
naming a record type, a map or a fixed-length array — still dispatches
([`module_discovery`](src/module_discovery.rs)). A Groot tree the binary
installs binds no module function: its nodes are the native control nodes.
[`module_dir`](src/module_dir.rs) reads module directories for an embedder.

The chain down to the bridge's code, in a `studio-bridge` build:

1. [`device_dir`](src/device_dir.rs) resolves the device directory
   (`device_dir::from_env`): `DEVICE_DIR`; else `<IDENTITY_FILE>_dir` while
   that deprecated variable is set; else
   `device_dir::of(<DEVICE_LOCAL_ID, or default>)`. The per-user directory
   needs a home: a container whose user has no passwd entry sets `DEVICE_DIR`.
   An embedder whose platform gives it a data directory (Android, a Tauri app)
   passes its own to `studio::connect_with_device_dir`.
2. [`studio::credentials`](src/studio/credentials.rs) names `<device dir>/studio`
   and hands it to `DeviceCredentials::in_dir`. arora never writes inside it;
   the files there are the client's.
3. `studio::connect` passes the `DeviceCredentials` to
   `ZenohDeviceClient::new_with_credentials`, or to
   `new_endpoint_with_credentials` when `STUDIO_BRIDGE_ENDPOINT` names the
   bridge's endpoint. The client's Firebase authenticator signs the device in
   with the refresh token the credentials hold and saves each rotated one there.

Credentials from arora 11.0 and earlier, in `.semio/arora` under the
executable's directory, the home directory or the current directory, move into
`devices/default/studio/` at the device's first start. A deprecated
`IDENTITY_FILE` migrates to `<IDENTITY_FILE>_dir`, which that run uses as its
device directory.

See the [root map](../../readme.md) for where this sits in Arora.

[`Arora`]: src/lib.rs
