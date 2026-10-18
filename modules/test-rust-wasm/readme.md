# Test Rust -> WASM module

This is a module written in Rust and compiled to WASM to be executed by arora-engine.
It relies on [wasmtime for Rust](https://docs.wasmtime.dev/lang-rust.html).

The module declares its Arora interface in Rust, with
[`arora-module`](../../crates/arora-module/readme.md)'s macros: the ids, the
header the engine loads it with, and the `arora_function_<id>` entry points
the executor calls all come from `src/lib.rs`.

This module exports symbols imported by the module
[Behavior Tree Nodes](https://github.com/semio-ai/arora-sdk),
and is used in the tests of the
[Behavior Tree library](https://github.com/semio-ai/arora-sdk).

Exported functions:

| Name | Signature | Notes |
|------|-----------|-------|
| `ping` | `() -> ()` | Does nothing |
| `succeed` | `() -> bool` | Always `true` |
| `cos` | `(angle: f32) -> f32` | Wraps `f32::cos` |
| `add` | `(a: f32, b: f32) -> f32` | Returns `a + b` |
| `text` | `(length: u32) -> String` | A string of `length` bytes |
| `window` | `(start_ns: u64, end_ns: Option<u64>) -> Option<u64>` | The window's length; `None` when `end_ns` is absent |
| `greet` | `(name: Option<String>) -> String` | `"hello, <name>"`, or `"hello"` when `name` is absent |
| `frame` | `(length: u32, revision: u64, time_ns: u64, positioned_by: &mut Option<u64>) -> Vec<f64>` | `length` values, value `i` being the time in seconds plus `i`, with `revision` written into `positioned_by` |
| `reading` | `(angle: f32) -> Reading` | The angle, its cosine and its `Quadrant` |
| `readings` | `(count: u32) -> Vec<Reading>` | The readings of `count` angles a quarter turn apart, from zero |

`Reading` (a structure, version 1.1.0) and `Quadrant` (an enumeration) derive
`AroraType`, so the module's declaration carries their records
(`test_rust_wasm::type_records`) beside its own (`test_rust_wasm::record`): a
device describes `reading` and `readings` only when it holds those records.
The module's records live in the `tests` scope (`TESTS`). `Reading` states its
scope, `trigonometry` (`TRIGONOMETRY`), where `Quadrant`, named only through
it, lives too.

The same artifact holds a second module, `compass` (`bearing(degrees: f32) ->
Reading`), in the `navigation` scope, so a device can load two modules of
different scopes sharing a type: both ship the same `Reading` and `Quadrant`
records.

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
