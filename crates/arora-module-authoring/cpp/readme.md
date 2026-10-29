# `arora-module-cpp`

Generates the C++ bindings of a module declared in a `module.yaml`. It is the
generator [`arora-module-cli`](../cli/readme.md) runs for `-l cpp`, and is
only an executable.

For a module `example-cpp`, it writes:

- `include/example-cpp.hpp` — the exported functions to implement, in the
  namespace `example_cpp`, every parameter a `const std::optional<T> &`;
- `source/example-cpp.cpp` — the code that decodes a call's argument buffer,
  calls the function and encodes its result, and the table
  `arora_module_exports` ([`arora/module.hpp`](../../../libs/cpp/include/arora/module.hpp))
  that `arora-cpp-component` serves through a component's `functions` and
  `dispatch` exports;
- `include/types/…`, `source/types/…` — a C++ type per structure and
  enumeration record the module names;
- per imported module, its header and source, whose functions call it through
  `arora_dispatch`;
- `records/module/<id>@<version>.yaml` — the module's record, which a module
  importing this one loads with `arora-module-cli --include`.

The generated code links against the C++ library in
[`libs/cpp`](../../../libs/cpp/CMakeLists.txt) and the `wasm32-wasip2` static
libraries of `arora-buffers` and `arora-util`. The
[example C++ module](../../../examples/modules/cpp/readme.md) is the whole
recipe, from `module.yaml` to the module directory a device loads;
[test-cpp-2](../../../modules/test-cpp-2/readme.md) imports a function of
[test-cpp](../../../modules/test-cpp/readme.md).
