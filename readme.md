# Semio Arora

Semio Arora is a C library (written in Rust) and associated tooling for executing behavior trees in a sandboxed environment.

## Prerequisites
  - Rust. You may need to add first the generic WebAssembly target:
    ```bash
    rustup target add wasm32-unknown-unknown
    ```
  - Python 3
  - CMake 3
  
### Windows

  - Ninja

## Build

```bash
./configure.py
cd build
make # or ninja on Windows
```

## Debug

To build rust crates in debug, you can configure it with:

```bash
./configure.py USE_RUST_DEBUG=1
```

To get backtraces from fatal errors in code generation tools, try:

```bash
RUST_BACKTRACE=& cmake --build build/
```

## Modules

Modules are the building blocks of Semio Arora. Each module exports symbols for other modules to use.
They can be implemented in C++ and in Rust, compiled into WebAssembly libraries.
The symbols available in a compiled module is described in a `module.yaml` file.
See [test-cpp](modules/test-cpp/module.yaml) or [test-wasm](modules/test-wasm/module.yaml)
for working examples.

Authors of modules should write a `module.yaml` file and
use `arora-module-cli` to generate the adequate sources to implement it.
`arora-module-cli` also produces a `module.yaml` file with named symbols stripped.
This is called a "header", and it is used by the runtime to identify the symbols.
Use `arora-cli --header <module.yaml> --exe <binary>` to try loading a module.