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
```

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
options the binary parses (the Groot file); flatten it into a larger CLI and
inject the results through the builder.

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
        └── studio/                the Studio connection's credentials, kept by
            ├── key                arora-studio-bridge-client's DeviceCredentials
            └── refresh_token
```

The local id tells apart the devices one user runs on one host. It is local
only: the name a device shows (`DEVICE_NAME`) can change without making it
another device, and the id Studio knows it by is assigned at its first sign-in.
Anything else a device keeps goes in a subdirectory of its own beside `studio/`.

The chain down to the bridge's code, in a `studio-bridge` build:

1. [`device_dir`](src/device_dir.rs) resolves the device directory: `DEVICE_DIR`,
   else `device_dir::of(<DEVICE_LOCAL_ID, or default>)`. An embedder whose
   platform gives it a data directory (Android, a Tauri app) passes its own to
   `studio::connect_with_device_dir`.
2. [`studio::credentials`](src/studio/credentials.rs) names `<device dir>/studio`
   and hands it to `DeviceCredentials::in_dir`. arora never writes inside it;
   the files there are the client's.
3. `studio::connect` passes `DeviceCredentials::refresh_token()` and `saver()` to
   `ZenohDeviceClient::new`, whose Firebase authenticator signs the device in
   with the token and calls the saver with each rotated one.

Credentials from arora 11.0 and earlier, in `.semio/arora` under the
executable's directory, the home directory or the current directory, move into
`devices/default/studio/` at the device's first start. A deprecated
`IDENTITY_FILE` migrates to `<IDENTITY_FILE>_dir`, which that run uses as its
device directory.

See the [root map](../../readme.md) for where this sits in Arora.

[`Arora`]: src/lib.rs
