# Rust Code Generator (`arora-module-rust`)

This crate provides both a library and an binary.

The binary is meant to be used in the context of
[`arora-module-cli`](../arora-module-cli/readme.md).

The library can provides the function
[`generate_sources`](src/lib.rs)
to generate Rust code from
[`ModuleAsset`s produced by `arora-module-core`](../arora-module-core/readme.md#asset).
See [the Test Rust WASM module](../../modules/test-rust-wasm/readme.md)
for a working example.