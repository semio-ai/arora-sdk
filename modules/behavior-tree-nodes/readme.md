# Behavior Tree Nodes

A Rust WASM module providing the core nodes for behavior trees.

## Requirements

You need Rust installed, and `cargo wasi` to be installed first.

```bash
$ cargo install cargo-wasi
```

For basic tests,
[have `wasmtime` installed](https://docs.wasmtime.dev/cli-install.html) too.

## Build

Build for the host machine, for development and testing:

```bash
$ cargo build [...]
```

Build into WASM so that it can be loaded by `wasmtime`,
and therefore by the Arora engine, which uses it:

```bash
$ cargo wasi build
```

It is also automatically built via the parent project (Arora engine),
via CMake targets that include the build for the host,
the build into WASM, and the tests.
The resulting `.wasm` executable should be found under
`<this_dir>/target/wasm32-wasi/<debug_or_release>/test_rust_wasm.wasm`.
