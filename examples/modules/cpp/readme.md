# Example C++ module

A module for the [Arora Engine](../../../crates/arora-engine/readme.md)
written in C++. It adds two integers and counts the sums it returned, and
builds into the same form as a Rust module: a `wasm32-wasip2` component in
the module directory a device loads it from.

## How it is made

- [`module.yaml`](module.yaml) declares the module's interface: its id, its
  functions and their parameters, each pinned by id.
- [`arora-module-cli`](../../../crates/arora-module-authoring/cli/readme.md)
  generates the C++ bindings from it: `example-cpp.hpp` declares the functions
  in the namespace `example_cpp`, and its generated source turns the engine's
  calls into calls to them. It also writes the module's header and record.
- [`src/module.cpp`](src/module.cpp) implements the functions.
- [`CMakeLists.txt`](CMakeLists.txt) builds the guest with the WASI SDK's
  `wasm32-wasip2` toolchain file, links it against `arora-buffers`,
  `arora-util`, [`arora-cpp`](../../../libs/cpp) and `arora-cpp-component`,
  which serves the generated functions through the `arora:module` world's
  `functions` and `dispatch` exports, and calls
  `arora_module_directory`, whose target `example-cpp-module-directory`
  builds the guest and writes the module directory.

The guest links as a WASI reactor (`-mexec-model=reactor`). It has no
`main`; the component calls its `_initialize` once, when it is instantiated,
which runs the C++ static constructors, and the module's globals then live
across calls — the `sums` vector in `src/module.cpp`.

## Build and run it

```bash
cargo build -p example-cpp
```

writes the module directory `target/<profile>/modules/example-cpp/`:

```text
example-cpp/
├── header.json        the module's header
├── example-cpp.wasm   the guest
└── records/           the module's record
```

A device loads it with `--module`, or from its device directory's `modules/`:

```bash
cargo run -p arora -- --module target/debug/modules/example-cpp
```

[`build.rs`](build.rs) is how cargo drives the build in this workspace. A
module built outside it runs the same CMake project with those cache
variables set by hand: `CMakeLists.txt` lists them.
