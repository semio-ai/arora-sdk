# Case: the wasm guest

Every Rust guest in the SDK (`test-rust-wasm`, `test-behavior-tree-nodes`,
vizij's animation module) gets its `#[no_mangle] arora_function_<uuid>`
exports and their buffer codec from `arora-module-rust`, through a `build.rs`.
This case is the same guest from the declaration: no `build.rs`, no generated
sources, the shim emitted by `#[export]` under `cfg(target_arch = "wasm32")`.

Exercised on [`case-bt-nodes`](prototype/cases/bt-nodes/) (executor `wasm`,
mutable parameters): built as a `cdylib` for `wasm32-wasip1`, loaded into a
real `WebAssemblyExecutor` with the declaration's `header()`.

## Today

`modules/test-rust-wasm/src/arora_generated/export.rs`: per export, a shim
that walks the argument buffer field by field with `BufferReader`, matches
each field id against the generated `*_RAW_ID`, deserializes with a per-type
generated codec, calls the function with `Option<T>` arguments, and writes the
result and mutated parameters back with `BufferWriter`. `arora.rs` declares
the `arora_dispatch` imports (and panics on the host); `mod.rs` stitches the
files. `Cargo.toml` carries `arora-module-core`, `arora-module-rust` and
`arora-registry` as build dependencies and `tokio` to run them.

## With the declaration macros

The `#[export]` on each function also emits:

```rust
#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn arora_function_c803889f_4757_4b56_908f_4b2b47041eff(input_addr: usize) -> usize {
    // size-prefixed buffer in → Value::Structure → check id → invoke(Call) → Value::Structure → size-prefixed buffer out
}
```

The marshalling is `invoke` — the same code the host closure runs. The wire
codec is `arora_buffers::serde_uuid`, the `Value` codec the engine's executors
use on their own side; `arora_buffer_alloc` / `arora_buffer_free` come from
`arora_buffers::alloc`. The crate declares `arora-buffers` as a
`wasm32`-only dependency and `crate-type = ["cdylib", "rlib"]`.

## Established

| Claim | Evidence |
|---|---|
| A declared module built for `wasm32-wasip1` exports one `arora_function_<uuid>` per export plus the allocator | `strings target/wasm32-wasip1/debug/case_bt_nodes.wasm` (four `arora_function_*`, `arora_buffer_alloc`, `arora_buffer_free`) |
| The real executor loads it with `header()` and dispatches | `host/tests/guest.rs::the_guest_shim_dispatches_through_the_wasm_executor` |
| A mutable parameter comes back over the wasm ABI | `host/tests/guest.rs::a_mutable_parameter_comes_back_from_the_guest` |
| A guest-side failure surfaces as a `CallError` with the parameter named | `host/tests/guest.rs::a_guest_error_is_reported_as_a_call_error` |

## Cost

One `Value` materialization per call on the guest (buffer → `Value` →
typed, and back), where the generated codec went buffer → typed directly. The
host path already pays it. If it ever matters, `arora_buffers::froto_serde`
(typed straight to the wire) is the drop-in for `invoke`'s edges.

## Not covered

`arora_dispatch` / `arora_dispatch_indirect` from the guest (a module calling
another module, or ticking a child) — the imports the generated `arora.rs`
declares. The [imports case](case-imports.md) covers the caller side on the
host; the guest-side import through `env::arora_dispatch` is not prototyped.
