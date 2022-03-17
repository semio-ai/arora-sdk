# Semio Arora

Semio Arora is a C library (written in Rust) and associated tooling for executing behavior trees in a sandboxed environment.

## Prerequisites
  - Rust. You need to add first some WebAssembly targets:
    ```bash
    rustup target add wasm32-unknown-unknown
    rustup target add wasm32-wasi
    ```
  - Rust WASI, for Rust WASM modules:
    ```bash
    $ cargo install cargo-wasi
    ```
  - Python 3
  - CMake 3
  
### Windows

  - Ninja

## Build

```bash
mkdir build
cmake ..
cmake --build .
```

## Debug

By default it builds in debug.

To get backtraces from fatal errors in code generation tools, try:

```bash
RUST_BACKTRACE=1 cmake --build .
```

## Release

To build in release, use:

```bash
cmake -DCMAKE_BUILD_TYPE=Release -DUSE_RUST_DEBUG=0 ..
```

## Modules

Modules are the building blocks of Semio Arora. Each module exports symbols for other modules to use.
They can be implemented in C++ and in Rust, compiled into WebAssembly libraries.
The symbols available in a compiled module is described in a `module.yaml` file.
See [test-cpp](modules/test-cpp/module.yaml) or [test-wasm](modules/test-rust-wasm/module.yaml)
for working examples.

Authors of modules should write a `module.yaml` file and
use `arora-module-cli` to generate the adequate sources to implement it.
`arora-module-cli` also produces a `module.yaml` file with named symbols stripped.
This is called a "header", and it is used by the runtime to identify the symbols.
Use `arora-cli --header <module.yaml> --exe <binary>` to try loading a module.

When a function is called (for instance by using `arora-cli --call`),
the call arguments are passed in via a structure which `id`
corresponds to the function to call,
and with arguments `args` represented as structure fields,
associating an `id` to a `value`.
The functions return a structure which `id`
corresponds to the function called.
The first field must be of the same `id` as the function
and contains the return value.
The remaining fields correspond to parameters that the call has mutated.

## Go deeper

- [Arora Buffers](crates/arora-buffers/readme.md),
  about how data is exchanged between modules.

- [Rust WASM test module](modules/test-rust-wasm/readme.md).

- [Arora CLI](crates/arora-cli/readme.md),
  for more info about the command-line tool.
