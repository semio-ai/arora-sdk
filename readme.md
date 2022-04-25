# Semio Arora

Semio Arora is a C library (written in Rust) and associated tooling for executing behavior trees in a sandboxed environment.

## Semio Records

This project relies on a notion of records of type
`Enumeration`, `Structure` or `Module`.
They are provided by the following Semio projects:

- [Semio Record](https://github.com/semio-ai/semio-record.git)
- [Semio Store RPC](https://github.com/semio-ai/semio-store-rpc.git)
- [Semio Client](https://github.com/semio-ai/semio-client.git)

They provide the interface to connect to a
[Semio Database](https://github.com/semio-ai/semio-db.git),
which collects the records of the assets produced by Semio users.

The database does not need to be specified and running at build time.
At runtime, you can specify it by providing a
[Semio Client Configuration](https://github.com/semio-ai/semio-client/blob/master/src/authentication.rs),
with the command-line option `--config`.
A config file is typically produced by [Semio Client (`semio-cli`)](https://github.com/semio-ai/semio-client.git),
and can be reused in this context.

## Arora Engine

The Arora Engine is capable of loading types (`Enumeration`s or `Structure`s)
and `Module`s compiled into WebAssembly modules.
It can run functions, and provide hooks for the modules to call functions
from the other modules (named `arora_dispatch`),
or anonymous functions registered on-the-fly
(named `arora_dispatch_indirect`).

The modules are described locally using a
[specific schema](crates/arora-schema/readme.md),
differing slightly from the `Module` data structure
provided in [Semio Record](https://github.com/semio-ai/semio-record.git).
See [modules](#modules).

The types (`Enumeration`s or `Structure`s) as
[Semio Records](https://github.com/semio-ai/semio-record.git),
usually available through a [registry](crates/arora-registry/readme.md).
They can be saved into files that can be included by command-line tools.

The main command-line tool is [`arora-cli`](crates/arora-cli/readme.md).
It is used to start an engine, load modules and run functions.
It is meant to be compiled into native bytecode,
and load module executables compiled into WebAssembly (wasm32).

## Modules

Modules are the building blocks of Semio Arora.
Each module exports symbols for other modules to use.
They can be implemented in C++ and in Rust, compiled into WebAssembly libraries.
The symbols available in a compiled module is described in a `module.yaml` file.
See [test-cpp-2](modules/test-cpp-2/module.yaml) or [test-wasm](modules/test-rust-wasm/module.yaml)
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

## Behavior Trees

This project includes
[a library to run behavior trees](crates/arora-behavior-tree/readme.md),
described with references to functions provided by Arora
[modules](#modules).

Such functions rely on basic types provided as a library by
[`arora-behavior-tree-types`](crates/arora-behavior-tree-types/readme.md),
so that Rust bindings can be generated for them using
[`arora-module-rust`](crates/arora-module-rust/readme.md).

They are also available in the YAML format in
[`arora-behavior-tree-types-yaml`](crates/arora-behavior-tree-types/readme.md),
so that Rust or C++ bindings can be generated using
[`arora-module-cpp`](crates/arora-module-cpp/readme.md).
See [`arora-registry`](crates/arora-registry/readme.md) to load them
for other uses.

## Full Project Layout

- [Arora Schema](crates/arora-schema/readme.md)
  defines the data formats used to communicate between modules,
  and to advertise them locally.

- [Arora Buffers](crates/arora-buffers/readme.md),
  provides Rust, C and C++ implementations to read and write buffers.
  Relies on the C / C++ libraries provided in [`libs`](libs).

- [`arora-util`](crates/arora-util/readme.md),
  provides Arora-related utilities for C libraries,
  written in Rust.

- [Arora Engine](crates/arora/readme.md),
  the library of the engine.

- [Arora Registry](crates/arora-registry/readme.md),
  to handle local and remote registry of
  [Semio Records](https://github.com/semio-ai/semio-record.git).

- [Arora CLI](crates/arora-cli/readme.md),
  the CLI tool to load modules and run functions.

- [`arora-vfs`](crates/arora-vfs/readme.md),
  provides a virtual file system mostly used in code generation.
  It is potentially useful for WebAssembly modules.

- Arora Module libraries:
  - [`arora-module-core`](crates/arora-module-core/readme.md):
    a library to analyze type and module declarations,
    and resolve them for code generation.
  - [`arora-module-cli`](crates/arora-module-cli/readme.md):
    a library to generate code from a module description.
    It finds the various code generators locally.
  - [`arora-module-rust`](crates/arora-module-rust/readme.md):
    the Rust code generator for modules.
    Also works as a library.
  - [`arora-module-cpp`](crates/arora-module-cpp/readme.md):
    the C++ code generator for modules.
    Only works as a executable.

- Modules:
  - [`test-cpp`](modules/test-cpp/readme.md):
    a module to test the C++ bindings.
  - [`test-cpp-2`](modules/test-cpp-2/readme.md):
    a module to test the C++ that depends on
    [`test-cpp`](modules/test-cpp/readme.md)
    and [`behavior-tree-types-yaml`](crates/arora-behavior-tree-types-yaml/readme.md).
  - [Rust WASM test module](modules/test-rust-wasm/readme.md):
    a module to test the Rust bindings, that depends on
    [`behavior-tree-types`](crates/arora-behavior-tree-types/readme.md).
  - [Behavior Tree Nodes](modules/behavior-tree-nodes/readme.md):
    an initial collection of behavior tree nodes as module functions.

- Behavior Tree:
  - [Types](crates/arora-behavior-tree-types/readme.md):
    basic types used in behavior trees.
  - [Types YAML](crates/arora-behavior-tree-types-yaml/readme.md):
    the same types serialized as YAML.
  - [Behavior Tree](crates/arora-behavior-tree/readme.md):
    the Arora-specific library to run behavior trees.

## Building

### Prerequisites

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

#### Repositories

You need a read access to the following repositories:

- [Semio Record](https://github.com/semio-ai/semio-record.git)
- [Semio Store RPC](https://github.com/semio-ai/semio-store-rpc.git)
- [Semio Client](https://github.com/semio-ai/semio-client.git)
  
#### Windows

- Ninja

### Build

```bash
mkdir build
cmake ..
cmake --build .
```

It will automatically download the [WASI C++ SDK](https://github.com/WebAssembly/wasi-sdk),
and configure the project to use it for C++ [modules](#modules).

#### Debug

By default it builds in debug.

To get backtraces from fatal errors in code generation tools,
try this from the build directory:

```bash
RUST_BACKTRACE=1 cmake --build .
```

#### Release

To build in release, use:

```bash
cmake -DCMAKE_BUILD_TYPE=Release -DUSE_RUST_DEBUG=0 ..
```
