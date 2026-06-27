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