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

See the [root map](../../readme.md) for where this sits in Arora.

[`Arora`]: src/lib.rs
