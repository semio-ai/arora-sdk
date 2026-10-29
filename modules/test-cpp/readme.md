# test-cpp

A C++ module the tests load, declared in [`module.yaml`](module.yaml) and
built as the [example C++ module](../../examples/modules/cpp/readme.md) is: a
`wasm32-wasip2` component, linked as a WASI reactor, in a module directory.

It exports:

| Function | Signature |
|---|---|
| `test_2_args` | `(a: u32, b: u32) -> bool` — always `true` |
| `test_optional` | `(a: Option<u32>) -> Option<u32>` — `a + 1`, or nothing when `a` is absent |

[`test-cpp-2`](../test-cpp-2/readme.md) imports `test_2_args`.

`cargo build -p test-cpp` runs [`build.rs`](build.rs), which drives
[`CMakeLists.txt`](CMakeLists.txt): `arora-module-cli` generates the bindings
from `module.yaml`, the WASI SDK builds the guest, and the target
`test-cpp-module-directory` writes `target/<profile>/modules/test-cpp/` —
`header.json`, `test-cpp.wasm`, and `records/`, which holds the module's
record that `test-cpp-2`'s build reads.
