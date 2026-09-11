# Case: behavior-tree nodes

`modules/test-behavior-tree-nodes` is the guest that exercises what the other
cases do not: **mutable parameters** (`set_str(variable: &mut str, value:
str)`), whose new value travels back as a mutated argument, and the control
nodes' `children: [TickId]` parameter — an array of structures in.

Prototype: [`prototype/cases/bt-nodes`](prototype/cases/bt-nodes/src/lib.rs)
declares four of its exports; [`prototype/cases/host/tests/bt_nodes.rs`](prototype/cases/host/tests/bt_nodes.rs)
dispatches them.

## Today

`module.yaml` declares the functions; `build.rs` generates the guest with
`arora-module-rust`, which for a mutable parameter deserializes into a local
`Option<T>`, passes `&mut`, and after the call writes the parameter back as a
structure field after the return value. `src/lib.rs` implements
`set_str(variable: &mut Option<String>, value: Option<String>)`.

## With the declaration macros

```rust
#[export(id = "c803889f-4757-4b56-908f-4b2b47041eff")]
pub fn set_str(
  #[param(id = "8fa2f965-1eb5-40d9-baca-8facef0d31a8")] variable: &mut String,
  #[param(id = "88438955-7872-44ad-8464-d636dc5fe26f")] value: String,
) -> Status { *variable = value; Status::Success }

#[export(id = "32246df6-ab5d-4f18-9221-23e28731de93")]
pub fn seq(#[param(id = "5b6e9515-dbcc-411d-bee9-3d8cba5fedda")] children: Vec<TickId>) -> Status { … }
```

`&mut T` declares `mutable: true` in the header and the signature; the host
closure decodes the argument, passes `&mut`, then pushes the value back under
the parameter's id in `CallResult::mutated` — the same contract the generated
guest honours on the wire. `TickId` is a `#[derive(AroraType, AroraValue)]`
struct with the record's ids.

## Established

| Claim | Evidence |
|---|---|
| The four declared exports equal the generator's, mutability and the `children` array included | `bt-nodes/tests/header.rs::the_declared_exports_reproduce_the_generated_ones`, `…::a_mutable_parameter_declares_mutable` |
| A mutable parameter is written back as a mutated argument under its id, and only it | `host/tests/bt_nodes.rs::a_mutable_parameter_is_written_back_as_a_mutated_argument`, `…::an_unset_variable_comes_back_empty` |
| `children` arrives as an array of `TickId` structures | `host/tests/bt_nodes.rs::children_arrive_as_an_array_of_tick_ids` |
| The same module, built as a wasm guest, dispatches through the real executor and returns the mutable parameter | `host/tests/guest.rs` — see the [guest case](case-test-rust-wasm.md) |
| A client stub writes the mutable parameter back into the caller's variable | `host/tests/client.rs::a_mutable_parameter_is_read_back_by_the_stub` |

## Not covered

Ticking a child: the guest calls `arora_dispatch_indirect(callable_id)` — the
engine's indirect-dispatch seam, which a host closure would reach through the
`CallBridge` it is given. The declaration does not yet hand a closure that
bridge; the basic control nodes are native in `arora-behavior-tree` now, so no
declared module needs it today.
