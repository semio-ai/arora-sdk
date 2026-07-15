# Test Rust -> WASM module

This is a module written in Rust and compiled to WASM to be executed by arora-engine.
It relies on [wasmtime for Rust](https://docs.wasmtime.dev/lang-rust.html).

It uses a code generation step using `arora-module-cli -l rust`,
and puts all the sources under `src/arora-generated`,
and provides bindings for the Arora engine.

This module exports symbols imported by the module
[Behavior Tree Nodes](https://github.com/semio-ai/arora-sdk),
and is used in the tests of the
[Behavior Tree library](https://github.com/semio-ai/arora-sdk).

Exported functions:

| Name | Signature | Notes |
|------|-----------|-------|
| `ping` | `() -> string` | Returns `"pong"` |
| `succeed` | `() -> Status` | Always `Status::Success` |
| `cos` | `(angle: f32) -> f32` | Wraps `f32::cos` |
| `add` | `(a: f32, b: f32) -> f32` | Returns `a + b` |

## Requirements

You need Rust installed, and `cargo component` to be installed first.

```bash
$ cargo install cargo-component
```

For basic tests,
[have `wasmtime` installed](https://docs.wasmtime.dev/cli-install.html) too.

## Build

Build for the host machine, for development and testing:

```bash
$ cargo build [...]
```

> **Note:**
> Building the module for the host works here because
> this module does not depend on other modules,
> and therefore not on [the Arora Engine](../../crates/arora-engine/readme.md)
> and its functions `arora_dispatch` and `arora_dispatch_indirect`.

Build into WASM so that it can be loaded by `wasmtime`,
and therefore by the Arora engine, which uses it:

```bash
$ cargo component build
```

It is also automatically built via the parent project (Arora engine),
via CMake targets that include the build for the host,
the build into WASM, and the tests.
The resulting `.wasm` executable should be found under
`<this_dir>/target/wasm32-wasip1/<debug_or_release>/test_rust_wasm.wasm`.

## Run

The build produces a library, which exported functions can be called with `wasmtime`.
For instance, to call the `ping()` function:

```bash
wasmtime --invoke arora_function_5f423ba9_d5f9_46d7_a9b5_fb7d28f99ea6 test_rust_wasm.wasm 0
```
