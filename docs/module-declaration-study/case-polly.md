# Case: polly

`modules/polly` is the simplest module the SDK ships that is written in Rust
and runs natively: two functions, one string parameter, a `Status` return.

## Today

`modules/polly/module.yaml` declares the module. `build.rs` loads the
behavior-tree type records, analyzes the YAML through a registry, and
regenerates `src/arora_generated/` on every build: the `#[no_mangle]
arora_function_<uuid>` shims, the buffer codec for `Status`, the `*_RAW_ID:
[u8; 16]` constants, and a stripped `module.yaml`. `src/lib.rs` implements
`say(text: Option<String>) -> Status` — `Option` because the generated shim
hands an absent parameter as `None`.

Build-time dependencies: `arora-module-core`, `arora-module-rust`,
`arora-registry`, `tokio`.

## With the declaration macros

Two forms, one module. Inline
([`prototype/cases/polly`](prototype/cases/polly/src/lib.rs)):

```rust
#[arora_module_derive::module(id = "a1a6bb9a-…", name = "polly", version = "0.1.0", …)]
pub mod polly {
  use super::Status;

  #[export(id = "e5a41333-…")]
  pub fn hello_world() -> Status { say("Hello, world!".to_string()) }

  #[export(id = "e1b4bda7-…")]
  pub fn say(#[param(id = "fb3787f2-…")] text: String) -> Status { … }
}
```

Out of line ([`prototype/cases/polly-outline`](prototype/cases/polly-outline/src/polly.rs)):
`mod polly;`, the same two functions in `polly.rs`, and at its bottom
`declare_module! { id = …, exports = [hello_world, say] }`.

`Status` is `#[derive(AroraType, AroraValue)]` with the ids
`arora_behavior::Status` pins — the derive replaces that crate's hand-written
`From`/`TryFrom<Value>`.

No `build.rs`, no generated directory, no build-time dependency, and **no
`arora-engine` dependency**: the host side is assembled in the device crate
([`prototype/cases/host`](prototype/cases/host/src/lib.rs)) with
`host_module!(case_polly::polly)`.

## Established

| Claim | Evidence |
|---|---|
| The declaration's ids are the YAML's ids | `polly/tests/header.rs::the_declared_ids_are_the_yaml_ids` (and the same for `polly-outline`) |
| `header()` reproduces the generator's header, export for export | `polly/tests/header.rs::the_declared_header_reproduces_the_generated_exports` (both forms) |
| The declaration keeps what the generator's header strips | `polly/tests/header.rs::the_declared_header_keeps_what_the_generator_strips` |
| Both forms are the same module: same ids, same dispatch | `host/tests/polly.rs::the_outline_declaration_is_the_same_module` |
| The host module dispatches through a real `Engine` | `host/tests/polly.rs::say_dispatches_through_the_engine` |
| A missing or mistyped argument fails the call by name | `host/tests/polly.rs::a_missing_parameter_fails_the_call_by_name`, `…::an_argument_of_the_wrong_type_fails_the_call` |
| Every export is described for `DescribeMethods` | `host/tests/polly.rs::every_export_is_described_with_its_frozen_signature` |
| The module crates build for `wasm32-wasip1` without the engine | `cargo build -p case-polly -p case-polly-outline --target wasm32-wasip1` |
| The declaration is also the store record: frozen exports by id, one dependency `Status@1.0.0`, YAML round-trip identical | `polly/tests/header.rs::the_declaration_is_also_the_store_record` |
| The declaration is its own client interface | `host/tests/client.rs::a_declared_module_is_its_own_client_interface` |

## Not covered on this case

The native artifact: polly's is a cdylib the `native` executor loads by
symbol name. The shim `#[export]` emits is gated to `wasm32`; the same shim
under a native cdylib is the one step this case does not take (and the native
executor reads the result size big-endian where the writer writes it
little-endian — see the [inventory](inventory.md#findings-beside-the-study)).
