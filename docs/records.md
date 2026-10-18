# Type records

A **type record** is a versioned, shareable declaration of a type or module:
a [`structure`], an [`enumeration`], a [`module`] (a set of typed functions),
or a [`folder`] that organizes them — the top folders being
[scopes](#scopes). Records are how Arora names things across
machines: every record has a stable UUID and a semantic version, and anything
that refers to another record does so by id + version.

Records live in [`arora_types::record`](https://docs.rs/arora-types/latest/arora_types/record/),
next to the value and module vocabulary — declaring a type and pinning which
version of it you mean are one workflow
(→ [design note](design_decisions.md#type-records-live-in-arora-types)).

## Unfrozen → frozen

A record is declared **unfrozen**: its references carry version
*requirements* ("any 1.x"). Freezing pins every reference to a concrete
version, producing the **frozen** form that goes on the wire and into files:

```text
unfrozen (version_req) ──(Resolver)──▶ frozen (version)
```

The [`Freeze`](https://docs.rs/arora-types/latest/arora_types/record/freeze/trait.Freeze.html)
trait does the walk; a
[`Resolver`](https://docs.rs/arora-types/latest/arora_types/record/freeze/trait.Resolver.html)
picks the newest version matching each requirement. Registries implement
`Resolver`; everything else stays store-agnostic.

## Factories

Code declares records with plain constructors and freezes them against a
registry. The behavior tree's own types are the canonical example
([`arora-behavior-tree-types`](../crates/arora-behavior-tree-types/readme.md)):

```rust
let status = registry
    .tag_enumeration(STATUS_ENUMERATION_ID, version, declare_status_enumeration(parent))
    .await?; // -> frozen Enumeration, registered and pinned
```

Generators then turn frozen records into language bindings
(`arora-module-rust`, `arora-module-cpp`) — the same records drive Rust and
C++ code generation.

## Scopes

The top folders are **scopes**. A scope is the `@handle` of the account — a
person or an organization — that publishes the records in it, as a scoped
name says: `@vizij/animation` is the module record named `animation` in the
`vizij` scope, at the path `vizij.animation`. A scope is one level deep, and
a name is unique within it: a registry refuses a second record under a path
another record holds.

A scope is declared once by its owner, in code, as a
[`Scope`](https://docs.rs/arora-types/latest/arora_types/record/scope/struct.Scope.html):
a fixed id and the handle, which is its folder's name. The id is not derived
from the handle, because a handle can be renamed and records name their
scope by id. `arora_types::record::scope::ARORA` is the `arora` scope.

A record published to the store states its scope. A local registry also
holds records with none, at the root: a module an operator installs by hand,
a test's.

## Records from Rust declarations

A type that derives `AroraType` is also its record:
`AroraType::record(default_scope)` is the structure or enumeration under
the type's id, at the version `#[arora(version = "…")]` gives (`1.0.0` by
default), and `register_records` collects it with the records of every type
it names, each pinned at that type's own version, and the scopes they live
in. A type with a fixed-length array field has no record: a record has no
fixed-length form.

**Scopes of types.**

- A module's own types take the module's scope: a module declared with
  `scope = SCOPE` has its record there, and the types its functions name
  that state no scope live there too, as do the types those name.
- A type shared by modules of different scopes states its own with
  `#[arora(scope = SCOPE)]`. Its record lives there whatever names it, so
  every module naming it ships the same record and they load together:
  `Status` and `TickId` live in the `arora` scope (`arora.Status`,
  `arora.TickId`).
- A type stating none that modules of two scopes name is two different
  records under one id and version, and a registry refuses the second.

```rust
pub const VIZIJ: Scope = Scope { id: VIZIJ_SCOPE_ID, handle: "vizij" };

#[arora_module::module(id = "…", name = "animation", version = "…", scope = crate::VIZIJ)]
pub mod animation { … }
```

A module declared with `arora-module` carries the records of the types its
functions name (`type_records()`) beside its module record
(`record(parent)`, under its scope's id). `LocalRegistry::add_declared_module`
adds both at the version the module's header carries, with each scope's
folder, and fails, naming them, when the module names a type without a
record, when a type's record differs from the one held at its id and version,
and when another record holds a type's name in its scope:

```rust
let mut registry = LocalRegistry::new();
registry.add_declared_module::<animation::Module>(header.version.clone().into()).await?;
write_records_to_yaml_dir(&registry, artifact_dir.join("records"))?;
```

## Record files

Frozen records serialize to YAML, one file per record version
(`records/structure/<uuid>@<version>.yaml`, …; a folder, which has no
version, as `records/folder/<uuid>.yaml`). This serde shape is a wire
contract with the Semio store and is pinned by golden tests
(→ [design note](design_decisions.md#the-frozen-serde-shape-is-a-wire-contract)).

`arora_registry::local_yaml` reads a record directory into a registry
(`load_records_from_yaml_dir`, or `load_records` from records already in
memory) and writes what a registry holds back out
(`write_records_to_yaml_dir`, `records_as_yaml`). The same records write the
same files each time.

## Registries

[`arora-registry`](../crates/arora-registry/readme.md) is the local registry:
it stores, tags (freezes) and looks up records, entirely in-process. It holds
one record per id and version: adding the record it already holds there
keeps it, and a different one is refused. `module_at` and `type_at` look a
record up by its exact id and version, from memory, and
`missing_dependencies` lists what a record depends on that the registry does
not hold. The
registries backed by Semio's hosted store live in the private
`arora-registry-remote` crate
(→ [design note](design_decisions.md#the-remote-registry-is-a-separate-private-crate)).

[`structure`]: https://docs.rs/arora-types/latest/arora_types/record/structure/
[`enumeration`]: https://docs.rs/arora-types/latest/arora_types/record/enumeration/
[`module`]: https://docs.rs/arora-types/latest/arora_types/record/module/
[`folder`]: https://docs.rs/arora-types/latest/arora_types/record/folder/
