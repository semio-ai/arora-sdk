# Case: calling a module — the client side

A module's `imports:` in `module.yaml` make `arora-module-rust` generate, per
imported function, a Rust function that builds the call buffer by id, calls
`arora_dispatch`, and parses the result — the client stubs. On the host,
callers write `Call { module_id, id, args }` by hand (`crates/vizij/src/device.rs`
tests, `vizij-arora-host`) or through a graph node's authored `param_ids`.

## With the declaration macros

Two sources for the same stubs:

- **From the declaring crate** — the declaration is its own client interface.
  `#[export]` emits a `call` per function and the aggregate re-exports them:
  ```rust
  let status = polly::client::say(&mut engine, "hello".to_string())?;
  let mut variable = "old".to_string();
  nodes::client::set_str(&mut engine, &mut variable, "new".to_string())?;  // variable == "new"
  ```
  Any `CallBridge` serves: the engine, a bridge, a graph's function host.
- **From a header alone** — for a module that is not a Rust declaration:
  ```rust
  arora_module_derive::module_from_header!(
    "…/modules/polly/src/arora_generated/module.yaml",
    types = ["325a5767-e344-4532-860e-0749bcf2e428" => Status]
  );
  // ids::MODULE, ids::say::TEXT, header(), NAME, and:
  let status = say(&mut engine, "hello".to_string())?;
  ```
  The header is read at expansion. Primitive parameter types map to Rust
  types; a user type maps through the `types` list by id, and one left
  unmapped is passed as a raw `Value`.

## Established

| Claim | Evidence |
|---|---|
| A declared module's stubs call it through the engine, return decoded | `host/tests/client.rs::a_declared_module_is_its_own_client_interface` |
| A stub reads a mutable parameter back | `host/tests/client.rs::a_mutable_parameter_is_read_back_by_the_stub` |
| A client built from the header alone has the same ids and calls the same module | `host/tests/client.rs::a_client_built_from_the_header_alone_calls_the_same_module` |
| An `f32` array parameter goes through a stub | `host/tests/skills.rs::look_at_takes_an_f32_array_through_the_stub` |

## Not covered

The **guest-side** import: a wasm module calling another through
`env::arora_dispatch`. The stub is bridge-agnostic, so the missing piece is a
`CallBridge` implemented over `arora_dispatch` inside the guest — the twin of
the generated `arora.rs`.
