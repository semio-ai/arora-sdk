# Case: the interpreter module

`crates/arora-behavior/src/interpreter_module.rs` is the host-only module the
runtime registers the behavior interpreter under: `LOAD(graph)`,
`EDIT(diff)`, `SPAWN(call, policy) -> TaskHandle`, `HALT(task)`. Ids are
consts (`61726f72-6100-…`, "arora" in ASCII plus a counter); the payloads
travel as serde types converted through `value_serde` (`Graph`, `GraphDiff`,
`Call`, `RunPolicy`, `TaskHandle`, `TaskId`); encode/decode helpers are
written by hand, per function.

## What the declaration macros do with it

The declaration form fits — four exports, argument ids, `Status`-free
returns — but the **types do not**: `Graph` holds `HashMap<Uuid, Node>` and
`HashMap<Uuid, String>`, and `#[derive(AroraType)]` rejects maps
(`arora-types-derive`: "`HashMap` fields are not supported by
#[derive(AroraType)] yet"). The record vocabulary has no map form either
(`FrozenTy` is primitive, scalar or array), so a described signature could
not carry `Graph` even if the derive produced it.

So this module stays as it is until the type model grows a map — which is
also what `Option` parameters wait on ([Q1](open-questions.md#q1)). It is the
one occurrence in the inventory the study leaves untouched, and the fact it
establishes is the boundary of the mechanism: **a declared module's boundary
types are what `AroraType` can describe**. Today that is primitives, arrays,
structures, unit enumerations and the dynamic `Value`.

## What it would take

`ty::low::TypeKind` and `FrozenTy` gaining a map form, the derive emitting it,
the `Value` plane carrying it (`Value::KeyValue` exists; a typed map does
not). Then `interpreter_module.rs` becomes a declaration like any other, and
its four hand-written codec pairs go.
