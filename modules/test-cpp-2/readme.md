# Test C++ Module #2

This is another example module for the
[Arora Engine](../../crates/arora/../../readme.md),
required for the automated tests of the project.

It also calls [`arora-module-cli`](../../crates/arora-module-authoring/cli/readme.md)
to generate the C++ bindings used in the implementation,
and [`arora-cli`](../../crates/arora-cli/readme.md)
to perform a sanity check of the module.

It imports a function from the [the first C++ test module](../test-cpp/readme.md).