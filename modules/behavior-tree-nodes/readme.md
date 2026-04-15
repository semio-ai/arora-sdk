# Behavior Tree Nodes

A Rust WASM module providing the core nodes for behavior trees.

It depends on the [Test Rust WASM Module](../test-rust-wasm/readme.md).

## Requirements

You need Rust installed, and `cargo component` to be installed first.

```bash
$ cargo install cargo-component
```

## Build

Build into WASM so that it can be loaded by `wasmtime`,
and therefore by the Arora engine, which uses it:

```bash
$ cargo component build
```

> This module cannot be built for the host because it depends
> on another module, and it expects functions
> `arora_dispatch` and `arora_dispatch_indirect` of
> [the Arora Engine](../../crates/arora/readme.md) to be available at build time.
> Adding the whole crate as a dependency would unreasonably heavy.

It is also automatically built via the parent project (Arora engine),
via CMake targets that include the build for the host,
the build into WASM, and the tests.
The resulting `.wasm` executable should be found under
`<this_dir>/target/wasm32-wasip1/<debug_or_release>/test_rust_wasm.wasm`.
