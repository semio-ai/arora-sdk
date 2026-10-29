# `arora-module-core`

Reads a module's `module.yaml`, resolves every type and module it names
against a [registry](../../arora-registry/readme.md), and hands a code
generator the result: the records the generated code needs, in the order it
needs them. It is the library under
[`arora-module-cli`](../cli/readme.md) and the generators
([`arora-module-rust`](../rust/readme.md), [`arora-module-cpp`](../cpp/readme.md)).

## From `module.yaml` to assets

A `module.yaml` is a module's interface written by hand: its id, name, version
and executor, the functions it exports and those of other modules it imports,
their parameters and types. Types are named — `u32`, `str`, `arora.Status` —
or given by id, and an import names its module by name or id
([`module::high::ModuleDefinition`](../../arora-types/src/module/high.rs)).

```rust
let mut registry = LocalRegistry::new();
// …add the records the module names: its types, the modules it imports…
let assets = analyze_module_from_path("module.yaml", &mut registry).await?;
```

[`analyze_module_from_path`](src/lib.rs) (or `analyze_module`, given the
parsed definition) resolves each name to its record, freezes the module, and
returns its [`ModuleAsset`](src/lib.rs)s, in order:

1. `Type` — every structure and enumeration the module's functions reach,
   directly or through other types' fields and variants, once each;
2. `Import` — each imported function, with the id, name and version of the
   module exporting it;
3. `Module` — the module itself as a frozen record, with its
   `ModuleMetadata`: the executor, author, license and description a record
   does not hold.

A module's dependencies are what its exports and imports name; the
`dependencies:` key of a `module.yaml` is not read. Every module an import
names, and every record a type names, must be in the registry first: a build
script adds them, or `arora-module-cli --include` loads them from record
files.

## The header

[`header::generate_header_file`](src/header.rs) writes the module's header
from its `Module` asset and imports, twice: `module.yaml`, the form
`arora-cli --header` reads, and `header.json`, the form a module directory
carries beside its artifact. A header names every type by id.

## Talking to a generator process

`arora-module-cli` runs each generator as its own process and frames what
crosses its standard streams with [`Writer`](src/lib.rs) and
[`Reader`](src/lib.rs): each message is a big-endian `u32` length followed by
that many bytes of JSON, and a zero length ends the stream.
