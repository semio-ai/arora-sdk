# The declaration macros

Five macros, one derive, two traits. Prototype:
[`prototype/arora-module-derive`](prototype/arora-module-derive/src/lib.rs) and
[`prototype/arora-module-support`](prototype/arora-module-support/src/lib.rs)
(the traits — in the SDK they belong in `arora-types`); exercised by every
case under [`prototype/cases`](prototype/cases/).

| | On | Produces |
|---|---|---|
| `#[export(id = "…")]` | a function, with `#[param(id = "…")]` on each parameter | the function unchanged, plus a hidden sibling module holding its ids, its header export, its frozen signature, its record dependencies, `invoke` (the marshalling), the **host closure**, the **client stub**, and — under `cfg(target_arch = "wasm32")` — the **guest shim** |
| `declare_module! { id = "…", name = "…", …, exports = [a, b] }` | anywhere the exports are in scope — typically the bottom of the module's file | `ids`, `header(executor)`, `record(parent)`, `host_functions()`, `client::{a, b}` bound to the module id, and a marker type `Module` implementing `AroraModule` |
| `#[module(id = "…", …)]` | an **inline** Rust module | sugar: scans the module for `#[export]` functions and injects `declare_module!` at its end |
| `module_from_header!("…/module.yaml", types = ["<uuid>" => Type, …])` | in a **consumer** crate | `ids`, `header()`, and one typed stub per export, from the resolved (`low`) header of a module that is not a Rust declaration |
| `host_module!(path)` / `host_module::<M>()` | in the **host** crate | a `HostModule` from a declared module — in the SDK, one generic function on `ModuleBuilder` |
| `#[derive(AroraValue)]` | a `#[derive(AroraType)]` struct or unit-variant enum | `From<T> for Value`, `TryFrom<Value> for T`, and the type's record version — to be folded into `AroraType`'s derive |

Two traits: `AroraModule` (`id()`, `header()`, `record(parent)`,
`host_functions()`) implemented by the marker, the twin of `AroraType`; and
`AroraTypeVersion` (`arora_type_version()`), which belongs on `AroraType`.

## The two declaration forms

Inline — the module is the declaration:

```rust
#[arora::module(id = "a1a6bb9a-334f-4617-a764-9a55817039d8", name = "polly", version = "0.1.0")]
pub mod polly {
  use super::Status;

  #[export(id = "e5a41333-4848-411f-878c-f1d662ebb4a0")]
  pub fn hello_world() -> Status { say("Hello, world!".to_string()) }

  #[export(id = "e1b4bda7-1c7b-4322-b9a0-552201b8a011")]
  pub fn say(#[param(id = "fb3787f2-2151-49ce-8b61-6274984558ea")] text: String) -> Status { … }
}
```

Out of line — `mod polly;` in `lib.rs`, and in `polly.rs` the same functions
followed by the aggregate naming them:

```rust
declare_module! {
  id = "a1a6bb9a-334f-4617-a764-9a55817039d8", name = "polly", version = "0.1.0",
  exports = [hello_world, say]
}
```

Both yield the same module — `case-host/tests/polly.rs::the_outline_declaration_is_the_same_module`.

## What one declaration serves

```text
                       ┌── header(executor)    → module.yaml, at export; the runtime's load
                       ├── record(parent)      → the store's frozen module record
#[export] fns  ──────► ├── ids::…              → the one home of every id
+ declare_module!      ├── host_functions()    → host_module::<M>()   (device crate)
                       ├── client::<fn>        → typed stubs over any CallBridge
                       └── arora_function_<id> → the wasm guest's exports (wasm32 only)
```

The **host closure and the guest shim share `invoke`**: arguments out of the
`Call` by parameter id, the Rust function, the `CallResult` back. Only the ABI
differs — a boxed closure the engine calls directly, or a `#[no_mangle]`
function reading a size-prefixed buffer and writing one back, in the `Value`
wire format (`arora_buffers::serde_uuid`) the engine's executors speak on
their side. No per-type buffer codec: the generated shims' per-type codec
existed to produce exactly the bytes the `Value` codec produces (ARORA-55
"conform wire layout to serde_uuid"), so a shim over the `Value` codec is what
the engine expects by definition.

The **client stub** is the same function seen from a caller: a `Call` built by
id, dispatched through any `CallBridge`, the return decoded — `&mut`
parameters read back from `mutated`. A Rust caller depends on the declaring
crate and uses `polly::client::say(&mut bridge, text)`; a caller of a module
that is not a Rust declaration gets the same stubs from its header with
`module_from_header!`. The stub is bound to the module id by the aggregate:
`#[export]` leaves a textually scoped `macro_rules!` per function, and
`declare_module!` invokes each with `ids::MODULE`.

**The executor is not the declaration's to name.** A declaration cannot know
whether it will be built native or wasm; the step that exports the artifact
does, and a description handed to `Engine::load_module` must carry it. So
`header(executor)` takes it from the exporter — the prototype's stand-in for
`Header::executor: Option<Executor>` ([Q10](open-questions.md#q10)) — and a
host-only module, which is linked rather than loaded, exports its interface as
`record(parent)`, which has no executor.

## Facts

**An attribute macro sees only an inline module.** On `mod polly;` the
compiler refuses (`E0658: file modules in proc macro input are unstable`) on
stable and the pinned nightly alike; under nightly's `proc_macro_hygiene` gate
the macro is handed `mod polly;` with `content == None`. Hence the
function-like aggregate, and `#[export]` being a real macro so each function's
declaration exists on its own.

**Ids are explicit, never hashed.** `#[module]`, `#[export]` and `#[param]`
each require `id = "<uuid>"`, validated at expansion.

**The type mapping is `#[derive(AroraType)]`'s.** A Rust primitive maps by
table to its well-known id, `PrimitiveKind` and `Value` variant. `Vec<T>` is
an array of `T`. `arora_types::value::Value` (or a `#[arora(keyvalue)]`
field) is the dynamic KeyValue type. Any other path is an `AroraType`
referenced by `arora_type_id()`, versioned by `arora_type_version()`, and
converted through `Into<Value>` / `TryFrom<Value>`. `&mut T` declares a
mutable parameter. `Option` and maps are rejected: the record vocabulary has
no optional or map form — see the [interpreter module](case-interpreter-module.md).

**Both vocabularies are emitted from one mapping.** A header parameter is a
`module::low::TypeRef` (id, no version); a described function's is a
`record::ty::FrozenTy` (id **and** version, from the type). The record's
`dependencies` are the same references, deduplicated.

**The SDK's `#[derive(AroraType)]` rejects `version` inside `#[arora(…)]`**
("unknown `arora` attribute"). The prototype carries it as
`#[arora_version("1.1.0")]`, a helper attribute its own derive declares; in the
SDK this is `#[arora(version = "…")]` once that derive's parser knows the key.

**Arguments are matched by id.** A missing one fails as ``missing parameter
`text` of `say` ``, a mistyped one as ``parameter `text` of `say`: unexpected
value …``, and reversed arguments give the same result
(`case-host/tests/animation.rs::arguments_are_matched_by_id_whatever_their_order`
— the property [VIZ-129](https://linear.app/semio-ai/issue/VIZ-129/the-animation-host-module-reads-call-arguments-by-position-not-by)
found broken).

**The derived conversions are wire-compatible with the generated ones.**
`#[derive(AroraValue)]` on `TrackOutput` produces the exact `Value::Structure`
the ARORA-55 generated `Into<Value>` produces (`case-animation/tests/conversions.rs`).
`value_serde`'s seeded walk was not usable for this: it refuses enumerations
(`walk.rs`: "enumeration types are not supported yet").

**The guest shim is what the executor calls.** A declared module built as a
`cdylib` for `wasm32-wasip1` exports `arora_function_<uuid>` per export, and
`arora_buffer_alloc` / `arora_buffer_free` — `#[no_mangle]` in
`arora_buffers::alloc`, kept in the cdylib from the dependency. The real
`WebAssemblyExecutor` loads it with the declaration's own `header()`,
dispatches, returns a mutable parameter, and reports a guest error as a
`CallError` (`case-host/tests/guest.rs`).

**The hidden module resolves the function's types through `use super::*`.**
Types written in the signature (`Status`, `AnimationClip`) resolve as they did
in the parent; a type named through `self::` would not — none is.

**A declared module crate builds as a wasm guest and carries no engine.**
Every module crate under `cases/` depends on `arora-types` (and, for
`wasm32`, `arora-buffers`); `cases/host` alone depends on `arora-engine`.

## Naming, as prototyped

`ids::MODULE`, `ids::<fn>::FUNCTION`, `ids::<fn>::<PARAM>`;
`header(executor)`, `record(parent)`, `host_functions()`, `client::<fn>`; the
marker `Module`. The hidden per-function module is `__arora_export_<fn>`, the
hidden per-function macro `__arora_client_<fn>!`.
