# C++ Code Generator (`arora-module-cpp`)

Produces C++ code to support the implementation of an Arora module in C++.
It is meant to be run in the context of
[`arora-module-cli`](../cli/readme.md).

The generated code depends on the C++ library `arora-cpp`.
It will produce C++ headers in the `include` directory,
and C++ source files in the `source` directory. The module's source defines
`arora_module_exports` (`arora/module.hpp`), the table of its functions that
`arora-cpp-component` serves through a component's `functions` and `dispatch`
exports.

See [the Test C++ module 1](../../../modules/test-cpp/readme.md)
and [2](../../../modules/test-cpp-2/readme.md) for working examples.
