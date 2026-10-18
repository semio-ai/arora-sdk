# Registry

A package manager for Arora.
The registry is meant to be a server provided
by the project `semio-db`.
This crate is the local, publishable core: a [`LocalRegistry`](src/local/mod.rs)
implements both the [`ReadableRegistry` and `EditableRegistry`](src/lib.rs)
traits, supports the addition of `Structure`, `Enumeration` and `Module`
on the fly, and provides a local index to look them up fast.

The registries backed by the remote server — `RemoteRegistry` (read-only,
using [`semio-client`](https://github.com/semio-ai/semio-client)) and
`RemoteCachedRegistry` (a remote coupled with a `LocalRegistry` cache) — live
in the private sibling crate `arora-registry-remote`, so this crate carries
no private git dependencies.

## Record versions

Programs using a registry should refer to records using version requirements,
if not explicit versions. Access to the latest non-tagged
(and therefore *unfrozen*) versions would result in potentially incompatible dependencies.

Use `EditableRegistry::tag_<record>` functions to produce tagged versions,
with dependencies frozen according to the existing tagged records.

Use `ReadableRegistry::get_<record>` functions to retrieve tagged versions.

A registry holds one record per id and version, and several versions of a
record side by side. Adding the record it already holds under an id and
version keeps it; a different record there is refused. `LocalRegistry::module_at`
and `LocalRegistry::type_at` look a record up by its exact id and version,
from memory, and `LocalRegistry::missing_dependencies` lists the records a
record depends on, transitively, that the registry does not hold.

`LocalRegistry::add_declared_module` adds the records of a module declared in
Rust: its module record, in the folder given, and the records of the types
its functions name, which `#[derive(AroraType)]` produces, each in the folder
its type declares (added with them) or, for a type declaring none, in the
module's. Modules sharing a type that declares its folder add together.

## YAML Records Layout

The records contained in a registry can be serialized into YAML
and organized in a directory, with respective sub-directories
for each record type:
- `enumeration`
- `folder`
- `module`
- `structure`
In each sub-directory, there is a file for each record,
named `<record_uuid>@<version>.yaml` (a folder, which has no version,
`<record_uuid>.yaml`).

The function [`load_records_from_yaml_dir`](src/local_yaml.rs)
can load such a directory and
feed an [`EditableRegistry`](src/lib.rs) with these records; `load_records`
loads the same records from memory. `write_records_to_yaml_dir` writes the
records a `LocalRegistry` holds in this layout, and `records_as_yaml` hands
them over as `load_records` reads them.
This is useful for defining types locally in an exchangeable format
to work with CLI tools like [`arora-cli`](../arora-cli/readme.md)
or [`arora-module-cli`](../arora-module-authoring/cli/readme.md).
