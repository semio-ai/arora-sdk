# Arora — Detailed Overview

> Preserved from the original root readme before the repo was consolidated into
> the single `arora-sdk` workspace. Some crate names/paths predate the
> consolidation and will be refreshed as the reorg lands; the concepts hold.

Semio Arora is a runtime dedicated to Semio's robotics software.

For a top-down tour of the codebase see [`docs/architecture.md`](architecture.md);
for the *why* behind the build setup and engine layout see
[`docs/design_decisions.md`](design_decisions.md).

## Arora Engine

The engine is the core component, capable of putting together heterogenous modules
under an uniform entry point.

Concretely, the Arora Engine is capable of loading `Module`s
(as defined in [Arora Types](https://docs.rs/arora-types/latest/arora_types/)
["low" `Module`](https://docs.rs/arora-types/latest/arora_types/module/low/struct.ModuleDefinition.html)),
and their binary payload, executed either [natively](../crates/arora-engine/src/executor/native.rs),
by [`wasmtime`](../crates/arora-engine/src/executor/wasm/mod.rs) or
a [browser host](../crates/arora-engine/src/executor/browser/mod.rs).

It loads and exposes the types declared in the modules (`Enumeration`s or `Structure`s),
functions, and provides hooks for the modules to call functions
from the other modules (named `arora_dispatch`),
or anonymous functions registered on-the-fly
(named `arora_dispatch_indirect`).

Note that the module description are described locally using the
[Arora Types crate](https://github.com/semio-ai/arora-types),
differing slightly from the `Module`, `Enumeration` or `Structure` data structures
provided in [Semio Record](https://github.com/semio-ai/semio-record).
See [modules](#modules) and [records](#semio-records)

The main command-line tool is [`arora-cli`](../crates/arora-cli/readme.md).
It is used to start an engine, load modules and run functions.
In browsers, the crate [`arora`](../crates/arora-engine/readme.md) (the engine's crate),
should already provide similar functions to start an engine.

## Modules

Modules are the building blocks of Semio Arora.
Each module exports symbols for other modules to use.
They can be implemented in C++ and in Rust, compiled into WebAssembly libraries.
The symbols available in a compiled module is described in a `module.yaml` file.
See [test-cpp](../modules/test-cpp/readme.md),
[test-cpp-2](../modules/test-cpp-2/readme.md) or
[test-rust-wasm](../modules/test-rust-wasm/readme.md)
for working examples.

Authors of modules should write a `module.yaml` file and
use `arora-module-cli` to generate the adequate sources to implement it.
`arora-module-cli` also produces a `module.yaml` file with named symbols stripped.
This is called a "header", and it is used by the runtime to identify the symbols.
Use `arora-cli --header <module.yaml> --exe <binary>` to try loading a module.

**Important:** The `module.yaml` file is the source of truth. Each module's `build.rs`
regenerates code in `src/arora_generated/` on every build. Manual edits to generated
files will be lost. To add or modify functions, edit `module.yaml` and run
`cargo clean -p <module-name>` to force regeneration. When importing functions from
other modules, add them to both the `imports:` and `dependencies:` sections.
See [`AGENTS.md`](../AGENTS.md) for detailed guidance on code generation.

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

## Semio Records

This project relies on a notion of records of type
`Enumeration`, `Structure` or `Module`.
They are provided by the following Semio projects:

- [Semio Record](https://github.com/semio-ai/semio-record)
- [Semio Store RPC](https://github.com/semio-ai/semio-store-rpc)
- [Semio Client](https://github.com/semio-ai/semio-client)

They provide the interface to connect to a
[Semio Database](https://github.com/semio-ai/semio-db),
which collects the records of the assets produced by Semio users.

The database does not need to be specified and running at build time.
At runtime, you can specify it by providing a
[Semio Client Configuration](https://github.com/semio-ai/semio-client/blob/master/src/authentication.rs),
with the command-line option `--config`.
A config file is typically produced by [Semio Client (`semio-cli`)](https://github.com/semio-ai/semio-client),
and can be reused in this context.

The types provided by [Semio Records](https://github.com/semio-ai/semio-record),
are usually made available through a [registry](../crates/arora-registry/readme.md).
They can be saved into files that can be included by command-line tools.

## Behavior Trees

This project includes
[a library to run behavior trees](https://github.com/semio-ai/arora-behavior-tree),
described with references to functions provided by Arora
[modules](#modules).

Such functions rely on basic types provided as a library by
[`arora-behavior-tree-types`](https://github.com/semio-ai/arora-behavior-tree),
so that Rust bindings can be generated for them using
[`arora-module-rust`](../crates/arora-module-rust/readme.md).

They are also available in the YAML format in
[`arora-behavior-tree-types-yaml`](https://github.com/semio-ai/arora-behavior-tree),
so that Rust or C++ bindings can be generated using
[`arora-module-cpp`](../crates/arora-module-cpp/readme.md).
See [`arora-registry`](../crates/arora-registry/readme.md) to load them
for other uses.

## Full Project Layout

- [Arora Types](https://github.com/semio-ai/arora-types)
  defines the data formats used to communicate between modules,
  and to advertise them locally. Published as an external crate.

- [Arora Buffers](../crates/arora-buffers/readme.md),
  provides Rust, C and C++ implementations to read and write buffers.
  Relies on the C / C++ libraries provided in [`libs`](../libs).

- [`arora-util`](../crates/arora-util/readme.md),
  provides Arora-related utilities for C libraries,
  written in Rust.

- [Arora Engine](../crates/arora-engine/readme.md),
  the library of the engine.

- [Arora Registry](../crates/arora-registry/readme.md),
  to handle local and remote registry of
  [Semio Records](https://github.com/semio-ai/semio-record).

- [Arora CLI](../crates/arora-cli/readme.md),
  the CLI tool to load modules and run functions.

- [Arora Web](https://github.com/semio-ai/arora-sdk),
  a `wasm-bindgen` entry point that hosts the engine inside a browser
  (browser-native `WebAssembly` instead of wasmtime).

- [`arora-vfs`](../crates/arora-vfs/readme.md),
  provides a virtual file system mostly used in code generation.
  It is potentially useful for WebAssembly modules.

- Arora Module libraries:
  - [`arora-module-core`](../crates/arora-module-core/readme.md):
    a library to analyze type and module declarations,
    and resolve them for code generation.
  - [`arora-module-cli`](../crates/arora-module-cli/readme.md):
    a library to generate code from a module description.
    It finds the various code generators locally.
  - [`arora-module-rust`](../crates/arora-module-rust/readme.md):
    the Rust code generator for modules.
    Also works as a library.
  - [`arora-module-cpp`](../crates/arora-module-cpp/readme.md):
    the C++ code generator for modules.
    Only works as a executable.

- Modules:
  - [`test-cpp`](../modules/test-cpp/readme.md):
    a module to test the C++ bindings.
  - [`test-cpp-2`](../modules/test-cpp-2/readme.md):
    a module to test the C++ that depends on
    [`test-cpp`](../modules/test-cpp/readme.md)
    and [`behavior-tree-types-yaml`](https://github.com/semio-ai/arora-behavior-tree).
  - [Rust WASM test module](../modules/test-rust-wasm/readme.md):
    a module to test the Rust bindings, that depends on
    [`behavior-tree-types`](https://github.com/semio-ai/arora-behavior-tree).
  - [Behavior Tree Nodes](https://github.com/semio-ai/arora-behavior-tree):
    an initial collection of behavior tree nodes as module functions.
  - [NAO](https://github.com/semio-ai/arora-sdk): a tentative module for NAO support.
  - [Polly](https://github.com/semio-ai/arora-sdk): a module providing nodes for AWS Polly TTS.

- Behavior Tree:
  - [Types](https://github.com/semio-ai/arora-behavior-tree):
    basic types used in behavior trees.
  - [Types YAML](https://github.com/semio-ai/arora-behavior-tree):
    the same types serialized as YAML.
  - [Behavior Tree](https://github.com/semio-ai/arora-behavior-tree):
    the Arora-specific library to run behavior trees.
