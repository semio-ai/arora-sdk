# test-rust-wasm

The reference Rust guest the tests load: a module declared in Rust with
[`arora-module`](../../crates/arora-module/readme.md), built as a
`wasm32-wasip2` component for the `wasm-component` executor and as a shared
library for the `native` one, from the same source.

Everything a host needs comes from [`src/lib.rs`](src/lib.rs): the module's
ids, its header (`test_rust_wasm::header(executor)`), its records, and the
component's exports (`arora_module::artifact!`).

## Functions

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

## Build

```bash
# the shared library, for the native executor
cargo build -p test-rust-wasm
# the component, for the wasm-component executor
cargo build -p test-rust-wasm --target wasm32-wasip2
```

The crates that test with it build it themselves, as an artifact dependency:
`arora-engine` loads the shared library through `NativeExecutor`, while
`arora`, `arora-web` and the integration tests load the component.
