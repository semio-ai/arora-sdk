# `arora-module-rust`

Generates the Rust bindings of a module declared in a `module.yaml`: its
types, its exported functions' entry points, and stubs for the functions it
imports. A Rust module whose interface is a Rust declaration needs none of
this — [`arora-module`](../../arora-module/readme.md) derives everything from
the declaration; this generator serves a Rust guest declared in a
`module.yaml`, such as one that calls other modules.

## Generate from a build script

A module's `build.rs` resolves its `module.yaml` with
[`arora-module-core`](../core/readme.md), generates the sources, and writes
them into `src/arora_generated/`
([`modules/test-behavior-tree-nodes/build.rs`](../../../modules/test-behavior-tree-nodes/build.rs)):

```rust
let assets = analyze_module_from_path("module.yaml", &mut registry).await?;
let sources = generate_sources(assets, &mut registry, &ExternTypes::new()).await?;
sources.sync("src/arora_generated/".into()).await?;
rustfmt::apply_rustfmt("src/arora_generated/").await?;
```

The crate then implements each exported function at its root, under the
export's name, and serves them from its artifact:

```rust
mod arora_generated;
arora_module::artifact!(arora_generated::export);

fn set_str(variable: &mut Option<String>, value: Option<String>) -> Status { … }
```

Every parameter reaches the function as an `Option`, `None` when the call
leaves it out; a mutable one as `&mut Option<T>`, whose value is returned with
the result.

## What it generates

[`generate_sources`](src/lib.rs) returns an
[`arora-vfs`](../../arora-vfs/readme.md) directory holding:

- `export.rs` — per export, its id constants, `dispatch_buffer_<id>` (a call
  as the artifact receives it) and `arora_function_<id>`, the entry point a
  shared library exports; and `function_ids()` and `dispatch_buffer(method,
  arg)`, which `arora_module::artifact!` serves as a component's `functions`
  and `dispatch`;
- a Rust type per structure and enumeration record, deriving
  `arora_types::AroraType` under the record's id, version and field or variant
  ids — the derive a hand-written type uses, so the two have the same API and
  the same `Value` conversions;
- per imported module, a stub per imported function;
- `arora.rs` — `arora_dispatch` and `arora_dispatch_indirect`, through which
  the stubs call other modules: over the `arora:module` world's `host`
  interface in a `wasm32-wasip2` component, and panicking stubs on the host,
  where the crate builds for its own tests;
- `module.yaml` and `header.json`, the module's header;
- `error.rs` and a `mod.rs` per directory.

[`generate_records`](src/lib.rs) writes the records of the types the module
names, under `structure/`, `enumeration/` and `folder/`, with a
`records.json` index. Call it before `generate_sources`, which consumes the
assets.

## Types declared elsewhere

A record that already has a Rust type is not generated. Declare it in
`ExternTypes` with the path generated code names it by:

```rust,ignore
let mut extern_types = ExternTypes::new();
extern_types.declare::<arora_behavior::Status>("arora_behavior::Status");
let sources = generate_sources(assets, &mut registry, &extern_types).await?;
```

The record must still be in the registry, so the module's declaration
resolves.

## The binary

`arora-module-rust` is also the generator
[`arora-module-cli`](../cli/readme.md) runs for `-l rust`: it reads the
assets on its standard input and writes the generated directory on its
standard output.
