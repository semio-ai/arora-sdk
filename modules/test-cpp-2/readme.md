# Test C++ Module #2

This is another example module for the
[Arora Engine](../../crates/arora/../../readme.md),
required for the automated tests of the project.

It also calls [`arora-module-cli`](../../crates/arora-module-authoring/cli/readme.md)
to generate the C++ bindings used in the implementation,
and [`arora-cli`](../../crates/arora-cli/readme.md)
to perform a sanity check of the module.

It imports a function from the [the first C++ test module](../test-cpp/readme.md).

It builds as [the first C++ test module](../test-cpp/readme.md) does, and
writes its module directory to `target/<profile>/modules/test-cpp-2/`: its
`records/` holds its own record and the records of the types its functions
name, from this directory's `records/`.
