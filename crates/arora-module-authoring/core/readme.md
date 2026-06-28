# `arora-module-core`

Core library underneath [`arora-module-cli`](../cli/readme.md).

It can be used separately: the functions
[`analyze_module` and `analyze_module_from_path`](src/lib.rs)
can read a [`ModuleDefinition`](https://github.com/semio-ai/arora-types)
(usually in a `module.yaml` file),
and resolve all its dependencies in the context of the given
[registry](../../arora-registry/readme.md).

They are translated into [`ModuleAsset`s](src/lib.rs),
arranged specifically to help generators in their work.
It can be used directly, like with
the [`arora-module-rust` library](../rust/readme.md),
or in a serialized form, like
[`arora-module-cli`](../cli/readme.md#communication-with-the-code-generator)
does.

See [the Test Rust WASM module](../../../modules/test-rust-wasm/readme.md)
for a working example using `arora-module-core` with
the [`arora-module-rust` library](../rust/readme.md)
