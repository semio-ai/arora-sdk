# arora

The **opinionated Arora runtime**. Where [`arora-engine`](../arora-engine) is the
bare, unopinionated engine, this crate gives you a batteries-included
[`Arora`] that:

- builds an engine with the **WebAssembly + native executors**,
- loads the **behavior-tree node module** (embedded at build time) along with the
  behavior-tree type records, and
- sits on the **Semio backend** ([`semio-record`]).

It can run a behavior tree handed to it at startup (as [Groot] XML); otherwise
it idles, waiting for behavior trees that will soon arrive over the bridge.

## Run it

`arora` embeds the behavior-tree node module (built automatically as a wasm
artifact), so there is nothing to build separately. Build it with the workspace
— it is a default member — and run the binary. Build it as a *member* (plain
`cargo build`), not via `cargo run -p arora`: as a standalone leaf it duplicates
some host crates across build contexts.

```sh
cargo build                      # builds arora into target/debug/arora

# run a Groot behavior tree at startup, then idle for trees over the bridge
./target/debug/arora crates/arora/examples/hello_tree.groot.xml

# or just idle
./target/debug/arora
```

## Use it as a library

```rust,no_run
use arora::Arora;

fn main() -> anyhow::Result<()> {
    // Async setup runs in a Tokio runtime; ticking a tree drives the wasm
    // executor and must happen outside it (after block_on returns).
    let rt = tokio::runtime::Runtime::new()?;
    let mut arora = rt.block_on(Arora::start())?;     // engine + behavior-tree module
    let xml = std::fs::read_to_string("tree.xml")?;
    let status = arora.run_groot_xml(&xml)?;          // run a tree…
    println!("{status:?}");
    arora.run_forever()                                // …then idle for bridge input
}
```

This is the layer the [`arora-cli`](../arora-cli) and
[`arora-web`](../arora-web) front-ends are meant to promote. See the
[root map](../../readme.md) for where this sits in Arora.

[Groot]: https://www.behaviortree.dev/
[`semio-record`]: https://github.com/semio-ai/semio-record
[`Arora`]: src/lib.rs
