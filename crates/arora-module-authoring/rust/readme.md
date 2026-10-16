# Rust Code Generator (`arora-module-rust`)

This crate provides both a library and an binary.

The binary is meant to be used in the context of
[`arora-module-cli`](../cli/readme.md).

The library can provides the function
[`generate_sources`](src/lib.rs)
to generate Rust code from
[`ModuleAsset`s produced by `arora-module-core`](../core/readme.md#asset).
See [the Test Rust WASM module](../../../modules/test-rust-wasm/readme.md)
for a working example.

## Generated types

Each enumeration and structure record becomes a Rust type deriving
`arora_types::AroraType` under the record's id, version, and variant or field
ids — the same derive a hand-written type uses, so a generated type and a
declared one have the same API and the same `Value` conversions. Export and
import shims carry a value of a record type as its `Value`, through
`arora_buffers::froto_value`.

A record that already has a Rust type is not generated. Declare it in
`ExternTypes` with the path generated code names it by, and pass that to
`generate_sources`:

```rust,ignore
let mut extern_types = ExternTypes::new();
extern_types.declare::<arora_behavior::Status>("arora_behavior::Status");
let sources = generate_sources(assets, &mut registry, &extern_types).await?;
```

The record must still be in the registry, so the module's declaration resolves.
