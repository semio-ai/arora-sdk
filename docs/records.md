# Type records

A **type record** is a versioned, shareable declaration of a type or module:
a [`structure`], an [`enumeration`], a [`module`] (a set of typed functions),
or a [`folder`] that organizes them. Records are how Arora names things across
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

## Record files

Frozen records serialize to YAML, one file per record version
(`records/structure/<uuid>@<version>.yaml`, …). This serde shape is a wire
contract with the Semio store and is pinned by golden tests
(→ [design note](design_decisions.md#the-frozen-serde-shape-is-a-wire-contract)).

## Registries

[`arora-registry`](../crates/arora-registry/readme.md) is the local registry:
it stores, tags (freezes) and looks up records, entirely in-process. The
registries backed by Semio's hosted store live in the private
`arora-registry-remote` crate
(→ [design note](design_decisions.md#the-remote-registry-is-a-separate-private-crate)).

[`structure`]: https://docs.rs/arora-types/latest/arora_types/record/structure/
[`enumeration`]: https://docs.rs/arora-types/latest/arora_types/record/enumeration/
[`module`]: https://docs.rs/arora-types/latest/arora_types/record/module/
[`folder`]: https://docs.rs/arora-types/latest/arora_types/record/folder/
