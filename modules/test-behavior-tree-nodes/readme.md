# Behavior Tree Nodes

A Rust WASM test module providing behavior-tree nodes.

The control nodes and the string nodes (`set_str`, `unset_str`, `is_str_set`,
`wait_str_set`, `regex_match`) it exports are core nodes, native to
`arora-behavior-tree` under the same ids: a tree dispatches them natively and
never reaches this module's copies. `status_identity`, `store`, `increase` and
`cos` (Groot `Status`, `Store`, `Increase`, `Cos`) are only here.

It depends on the [Test Rust WASM Module](https://github.com/semio-ai/arora-sdk).

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
> [the Arora Engine](https://github.com/semio-ai/arora-sdk) to be available at build time.
> Adding the whole crate as a dependency would unreasonably heavy.

It is also automatically built via the parent project (Arora engine),
via CMake targets that include the build for the host,
the build into WASM, and the tests.
The resulting `.wasm` executable should be found under
`<this_dir>/target/wasm32-wasip2/<debug_or_release>/test_behavior_tree_nodes.wasm`.
