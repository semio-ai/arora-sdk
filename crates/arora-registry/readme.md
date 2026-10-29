# arora-registry

The type registry: it holds [records](../../docs/records.md) — versioned
declarations of structures, enumerations and modules — and answers them by id
or by path, at the version a caller asks for. Module tools resolve the types a
`module.yaml` names through it; a device describes its modules' functions from
it.

## What a registry holds

A registry holds each record under an **id** and a **version**, several
versions of one record side by side. Every record has a parent folder and a
name, so it also has a **path**: the names of its folders and its own, joined by
`.` — `arora.Status` is the `Status` enumeration in the `arora`
[scope](../../docs/records.md#scopes). A record's fields, variants or exported
functions are indexed under its path in turn (`arora.Status.Running`). The
root, `ROOT_ID`, is held without a record.

Two rules keep it consistent:

- **One record per id and version.** Adding the record already held there
  keeps it; adding a different one is refused (`DuplicateVersion`).
- **A name is unique within its folder.** A record at a path another record
  holds is refused (`DuplicateSelector`), whatever its version.

## Smallest use

```rust
use arora_registry::{local::LocalRegistry, local_yaml::load_records_from_yaml_dir, ReadableRegistry};
use arora_types::record::Selector;
use semver::VersionReq;

let mut registry = LocalRegistry::new();
load_records_from_yaml_dir("records", &mut registry).await?;
let status = registry
    .get_type(&Selector::Path("arora.Status".into()), &VersionReq::parse("^1")?)
    .await?; // the newest 1.x of the Status enumeration
```

[`LocalRegistry`](src/local/mod.rs) is an in-process registry, on every target
including the browser. It implements the two registry traits and the record
[`Resolver`](../arora-types/src/record/freeze.rs).

## Reading

[`ReadableRegistry`](src/lib.rs) answers a record named by a `Selector` — its
id, or its path:

- `get_type` — a type's frozen definition (`TypeDefinitionFrozen`), at the
  newest version matching the requirement. A primitive is answered without a
  record: a scalar by its id, any primitive by its name (`f64`, `str[]`).
- `get_module` — a module record, the same way.
- `resolve_path` and `resolve_id` — a path's id, an id's path.
- `resolve_tag` — the newest version matching a requirement.
- `type_of` — which kind of record a selector names.

`LocalRegistry` also answers from memory, at an exact version: `module_at` and
`type_at` look a record up by id and version, and `missing_dependencies` lists
the records a record depends on, transitively, that the registry does not hold.

## Adding

[`EditableRegistry`](src/lib.rs) adds records. Each one's parent must be held
first.

- `add_structure`, `add_enumeration`, `add_module` and `add_folder` add a
  frozen record under an id and a version (a folder has no version).
- `tag_structure`, `tag_enumeration` and `tag_module` take an unfrozen record,
  freeze it — each reference pinned to the newest version the registry holds
  that matches its requirement — add it, and return the frozen form.

`LocalRegistry::add_declared_module` adds the records of a module declared in
Rust (`arora_types::module::declared::AroraModule`): its module record, in the
scope its declaration states; the records of the types its functions name,
transitively, which `#[derive(AroraType)]` produces, each in the scope its type
states or, stating none, in the module's; and the folder of each scope. It
fails, naming them, when the module names a type without a record of its own,
when a type's record differs from the one held at its id and version — a type
modules of different scopes share without stating its scope — and when another
record holds a type's path.

## Record files

The records a registry holds serialize to YAML, one file per record version,
in a directory per kind ([example](test_data/behavior_tree_types)):

```text
records/
├── folder/<uuid>.yaml
├── enumeration/<uuid>@<version>.yaml
├── structure/<uuid>@<version>.yaml
└── module/<uuid>@<version>.yaml
```

[`local_yaml`](src/local_yaml.rs) reads and writes this layout:

- `load_records_from_yaml_dir` adds a directory's records to any
  `EditableRegistry` — folders each after its parent, then enumerations,
  structures and modules, whatever order the files come in;
- `load_records` does the same from records in memory, `(RecordKind, file
  stem, YAML)` triples, for a target with no filesystem;
- `write_records_to_yaml_dir` writes what a `LocalRegistry` holds, and
  `records_as_yaml` hands it over as `load_records` reads it. The same records
  write the same files each time.

A module directory carries its records in this layout, and `arora-cli` and
`arora-module-cli` read record directories given to them.

## Semio's hosted store

Registries backed by Semio's hosted record store live in the separate, private
`arora-registry-remote` crate, which depends on Semio's client: Semio
integration is opt-in, and this crate carries no private dependency. Its
`RemoteRegistry` reads from the store; `RemoteCachedRegistry` reads through a
`LocalRegistry` cache and adds records to that cache only. Both implement the
same traits, so a consumer swaps registries without code changes.
