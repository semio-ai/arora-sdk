# Registry

A package manager for Arora.
The registry is meant to be a server provided
by [the project `semio-db`](https://github.com/semio-ai/semio-db).
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

## YAML Records Layout

The records contained in a registry can be serialized into YAML
and organized in a directory, with respective sub-directories
for each record type:
- `enumeration`
- `folder`
- `structure`
In each sub-directory, there is a file for each record,
named `<record_uuid>@<version>.yaml`.

The function [`load_records_from_yaml_dir`](src/local_yaml.rs)
can load such a directory and
feed an [`EditableRegistry`](src/lib.rs) with these records.
This is useful for defining types locally in an exchangeable format
to work with CLI tools like [`arora-cli`](../arora-cli/readme.md)
or [`arora-module-cli`](../arora-module-authoring/cli/readme.md).
