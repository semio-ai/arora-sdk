# test-rust-component

A guest that implements the `arora:module` world
([`crates/arora-engine/wit/arora-module.wit`](../../crates/arora-engine/wit/arora-module.wit))
by hand, with `wit-bindgen` and no Arora declaration: the engine's tests use
it to hold both component executors — wasmtime's and the browser's — to the
same behavior at the edges a module declared with
[`arora-module`](../../crates/arora-module/readme.md) never reaches.

Each function takes and returns raw bytes:

| Function | Id | What it does |
|---|---|---|
| `echo` | `dd6a4f61-9f88-4376-9d50-1e92e95a73aa` | returns its argument |
| `call-indirect` | `1bd1cf09-e94c-4c14-b653-32bd96157c41` | calls the callable whose id is its first 8 bytes, through `host.dispatch-indirect` |
| `call` | `473a50b2-f275-4767-989c-69aa2c2def5b` | calls a module's function through `host.dispatch`: a module id, a function id, then the argument |
| `wasi` | `0455052e-bb89-41ec-a963-2bbe82ea583b` | calls each WASI 0.2 interface a host provides and returns what it saw, a `name=value` line each |
| `trap` | `42681638-6fe3-4004-92fb-bb048dc3bbab` | panics, which traps the guest |
| `error-buffer` | `2041d791-4440-4dc9-b54d-1004e314cf99` | returns an error buffer whose message is its argument |
| `spin` | `5d0c4a3e-7b21-4f6e-9a8d-2c41b6e0f3a1` | loops until the host interrupts it |
| `grow` | `9e7b1c52-0d3a-4b8f-8e61-f4a2c9d7b053` | tries to reserve 64 MiB and returns `[1]` if it could, `[0]` if not |

Instantiating it runs `functions`, which first calls `spin` of the module
`6f3a9c2e-1b84-4d07-a5e2-8c9d0b7f4e16` through `host.dispatch`, ignoring the
result: with no module loaded under that id the call fails at once, and a
test that loads one there makes the guest's instantiation call a guest that
loops.

It builds only for `wasm32-wasip2`; the tests that load it
(`arora-engine`'s browser tests and the integration tests' `component.rs`)
build it as an artifact dependency:

```bash
cargo build -p test-rust-component --target wasm32-wasip2
```
