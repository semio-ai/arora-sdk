# Test C++ Module #1

This is an example module for the
[Arora Engine](../../crates/arora-engine/readme.md),
required for the automated tests of the project.
It demonstrates the implementation of a module in C++,
using CMake and the WASI SDK (managed at the root of the repository).

It calls [`arora-module-cli`](../../crates/arora-module-authoring/cli/readme.md)
to generate the C++ bindings used in the implementation.
It calls [`arora-cli`](../../crates/arora-cli/readme.md)
to perform a sanity check of the module.

It exports a function that [the second C++ test module](../test-cpp-2/readme.md)
should try to import.

It builds for `wasm32-wasip2` as a component of the `arora:module` world, a
WASI reactor, as the Rust guests build, and its build writes the module directory a device loads it from
(`arora --module`): `target/<profile>/modules/test-cpp/` holds `header.json`,
`test-cpp.wasm` and `records/`.