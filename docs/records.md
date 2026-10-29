# Type records

A **record** is how Arora names a type or a module so that programs which share
no source agree on it: a versioned declaration of a structure, an enumeration
or a module, under a stable UUID. Records live in
[`arora_types::record`](https://docs.rs/arora-types/latest/arora_types/record/),
next to the value and module vocabulary — declaring a type and pinning which
version of it you mean are one workflow
(→ [design note](design_decisions.md#type-records-live-in-arora-types)).

## What a record is

There are four kinds:

- a [`structure`] — a named type's fields, each a name and a type under the
  field's id, in declared order;
- an [`enumeration`] — a named type's variants, each a name and a payload type
  under the variant's id, in declared order;
- a [`module`] — a module's exported functions, each with its parameters (a
  name, a type and whether it is `mutable`) in declared order and its return
  type, and the records the module depends on;
- a [`folder`] — a node of the tree the others are organized in. The top
  folders are [scopes](#scopes).

A structure, an enumeration or a module record is held under an **id** and a
semantic **version**; a folder has no version. Every record has a parent folder
and a name, so it also has a **path**: its folders' names and its own, joined by
`.` (`arora.Status`). Anything that refers to another record does so by id and
version.

The type of a field, a variant's payload or a parameter is a type expression
([`record::ty`](https://docs.rs/arora-types/latest/arora_types/record/ty/)): a
primitive (`f64`, `str[]`), a reference to a structure or enumeration record,
an array of such a reference, or an option of any of these. A record has no
fixed-length array and no map.

## Unfrozen → frozen

A record is declared **unfrozen**: its references carry version
*requirements* ("any 1.x"). Freezing pins every reference to a concrete
version, producing the **frozen** form that record files and the store carry:

```text
unfrozen (version_req) ──(Resolver)──▶ frozen (version)
```

The [`Freeze`](https://docs.rs/arora-types/latest/arora_types/record/freeze/trait.Freeze.html)
trait does the walk; a
[`Resolver`](https://docs.rs/arora-types/latest/arora_types/record/freeze/trait.Resolver.html)
picks the version each requirement resolves to — a registry's picks the newest
it holds that matches. Registries implement `Resolver`; everything else stays
store-agnostic.

## Declaring records

**From a Rust type.** A type deriving `AroraType` is also its record:
`AroraType::record(default_scope)` is the structure or enumeration under the
type's id, at the version `#[arora(version = "…")]` gives (`1.0.0` by
default), and `register_records` collects it with the records of every type it
names, each pinned at that type's own version, and the scopes they live in. A
type with a fixed-length array field has no record.

**From a module declared in Rust.** A module declared with
[`arora-module`](../crates/arora-module/readme.md) carries its module record
(`record(parent)`) and the records of the types its functions name
(`type_records()`). `LocalRegistry::add_declared_module` adds both, at the
version the module's header carries, with each scope's folder:

```rust
#[arora_module::module(
    id = "76697a69-6a00-0000-0d00-000000000000",
    name = "animation",
    version = "3.0.0",
    scope = arora_types::record::scope::ARORA,
    // …
)]
pub mod animation { /* … */ }

let mut registry = LocalRegistry::new();
registry.add_declared_module::<animation::Module>(semver::Version::new(3, 0, 0)).await?;
write_records_to_yaml_dir(&registry, artifact_dir.join("records"))?;
```

It fails, naming them, when the module names a type without a record, when a
type's record differs from the one held at its id and version, and when another
record holds a type's name in its scope.

**By hand.** Code can build an unfrozen record and freeze it against a
registry, which pins its references to the records the registry holds:

```rust
use arora_behavior::{declare_status_enumeration, STATUS_ENUMERATION_ID, STATUS_ENUMERATION_VERSION};
use arora_types::record::scope::ARORA;

registry.add_folder(ARORA.id, ARORA.record()).await?;
let status = registry
    .tag_enumeration(STATUS_ENUMERATION_ID, STATUS_ENUMERATION_VERSION, declare_status_enumeration(ARORA.id))
    .await?; // the frozen Enumeration, registered under its id and version
```

**From a `module.yaml`.** Module authoring
([`arora-module-core`](../crates/arora-module-authoring/core/readme.md))
resolves the names a `module.yaml` uses against a registry into the module's
record, and gathers the records of the types it reaches. The generators
(`arora-module-rust`, `arora-module-cpp`) turn those frozen records into
language bindings — the same records drive Rust and C++ code generation.

## Scopes

The top folders are **scopes**. A scope is the `@handle` of the account — a
person or an organization — that publishes the records in it, as a scoped
name says: `@arora/animation` is the module record named `animation` in the
`arora` scope, at the path `arora.animation`. A scope is one level deep, and a
name is unique within it: a registry refuses a second record under a path
another record holds.

A scope is declared once by its owner, in code, as a
[`Scope`](https://docs.rs/arora-types/latest/arora_types/record/scope/struct.Scope.html):
a fixed id and the handle, which is its folder's name. The id is not derived
from the handle, because a handle can be renamed and records name their scope
by id. `arora_types::record::scope::ARORA` is the `arora` scope.

A record published to the store states its scope. A local registry also holds
records with none, at the root: a module an operator installs by hand, a
test's.

Which scope a type's record lives in:

- A module's own types take the module's scope: a module declared with
  `scope = SCOPE` has its record there, and the types its functions name that
  state no scope live there too, as do the types those name.
- A type shared by modules of different scopes states its own with
  `#[arora(scope = SCOPE)]`. Its record lives there whatever names it, so
  every module naming it ships the same record and they load together:
  `Status` and `TickId` live in the `arora` scope (`arora.Status`,
  `arora.TickId`).
- A type stating none that modules of two scopes name is two different
  records under one id and version, and a registry refuses the second.

## Record files

Frozen records serialize to YAML, one file per record version, in a directory
per kind: `records/structure/<uuid>@<version>.yaml`, and likewise
`enumeration/` and `module/`; a folder, which has no version, as
`records/folder/<uuid>.yaml`. The `Status` enumeration, in
`records/enumeration/325a5767-e344-4532-860e-0749bcf2e428@1.0.0.yaml`:

```yaml
parent: 61726f72-6100-0000-f01d-000000000000   # the arora scope
name: Status
variants:
  766e9e9a-446d-4e46-83e6-14b7ca101169:
    name: Success
    type:
      type: primitive
      value:
        kind: unit
  2468f46c-bb60-425c-9a4d-9ad326ccc7e2:
    name: Failure
    type:
      type: primitive
      value:
        kind: unit
  acd79ec6-0c44-401a-82f8-5da5422d3eec:
    name: Running
    type:
      type: primitive
      value:
        kind: unit
```

This serde shape is a wire contract with the Semio store and is pinned by
golden tests (→ [design note](design_decisions.md#the-frozen-serde-shape-is-a-wire-contract)).

`arora_registry::local_yaml` reads a record directory into a registry
(`load_records_from_yaml_dir`, or `load_records` from records already in
memory) and writes what a registry holds back out
(`write_records_to_yaml_dir`, `records_as_yaml`). The same records write the
same files each time.

## Registries

A registry holds records and answers them by id or path, at a version.
[`arora-registry`](../crates/arora-registry/readme.md) is the local one: it
stores, tags (freezes) and looks up records in-process. It holds one record
per id and version: adding the record it already holds there keeps it, and a
different one is refused. `module_at` and `type_at` look a record up by its
exact id and version, from memory, and `missing_dependencies` lists what a
record depends on that the registry does not hold. The registries backed by
Semio's hosted store live in the private `arora-registry-remote` crate
(→ [design note](design_decisions.md#the-remote-registry-is-a-separate-private-crate)).

[`structure`]: https://docs.rs/arora-types/latest/arora_types/record/structure/
[`enumeration`]: https://docs.rs/arora-types/latest/arora_types/record/enumeration/
[`module`]: https://docs.rs/arora-types/latest/arora_types/record/module/
[`folder`]: https://docs.rs/arora-types/latest/arora_types/record/folder/
