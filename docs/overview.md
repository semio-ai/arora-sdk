# Arora — Detailed Overview

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

Module descriptions and the record forms they resolve to are both defined in
[`arora-types`](https://docs.rs/arora-types/latest/arora_types/).
See [modules](#modules) and [records](#type-records).

The main command-line tool is [`arora-cli`](../crates/arora-cli/readme.md).
It is used to start an engine, load modules and run functions.
In browsers, [`arora-web`](../crates/arora-web/readme.md) hosts the engine on
the browser's native `WebAssembly` runtime and exposes the same operations to
JavaScript.

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

## Type records

Types and modules are declared as **type records** — versioned records of
`Enumeration`, `Structure` or `Module` defined in
[`arora_types::record`](https://docs.rs/arora-types/latest/arora_types/record/)
and served through a [registry](../crates/arora-registry/readme.md).
See [`docs/records.md`](records.md) for the model (unfrozen/frozen, freezing,
record files).

Records of the assets produced by Semio users are collected by Semio's hosted
store; connecting to it is the job of the private `arora-registry-remote`
crate and the opinionated `arora`/`arora-cli` layer (runtime `--config`,
produced by Semio Client). No store is needed at build time.
They can be saved into files that can be included by command-line tools.

## Behavior Trees

This project includes
[a library to run behavior trees](../crates/arora-behavior-tree/readme.md),
described with references to functions provided by Arora
[modules](#modules). The basic control nodes (sequence, fallback, …) are
dispatched natively by the library itself, so a tree of them runs without
loading any module.

Node functions rely on basic types provided as a library by
[`arora-behavior-tree-types`](../crates/arora-behavior-tree-types/readme.md),
so that Rust bindings can be generated for them using
[`arora-module-rust`](../crates/arora-module-authoring/rust/readme.md).

They are also available in the YAML format in
[`arora-behavior-tree-types-yaml`](../crates/arora-behavior-tree-types-yaml/readme.md),
so that Rust or C++ bindings can be generated using
[`arora-module-cpp`](../crates/arora-module-authoring/cpp/readme.md).
See [`arora-registry`](../crates/arora-registry/readme.md) to load them
for other uses.

## Full Project Layout

- [Arora Types](../crates/arora-types/readme.md)
  defines the data formats used to communicate between modules,
  and to advertise them locally — including the shared
  [`DataStore`](architecture.md#runtime-store-hal-bridge-behavior) vocabulary
  (`Key`, `State`, `StateChange`).

- [Arora Buffers](../crates/arora-buffers/readme.md),
  provides Rust, C and C++ implementations to read and write buffers.
  Relies on the C / C++ libraries provided in [`libs`](../libs).

- [`arora-util`](../crates/arora-util/readme.md),
  provides Arora-related utilities for C libraries,
  written in Rust.

- [Arora Engine](../crates/arora-engine/readme.md),
  the library of the engine.

- [Arora Registry](../crates/arora-registry/readme.md),
  the local registry of [type records](records.md).

- [Arora CLI](../crates/arora-cli/readme.md),
  the CLI tool to load modules and run functions.

- [Arora Web](../crates/arora-web/readme.md),
  a `wasm-bindgen` entry point that hosts the engine inside a browser
  (browser-native `WebAssembly` instead of wasmtime).

- The device runtime around the engine — see the
  [runtime architecture](architecture.md#runtime-store-hal-bridge-behavior):
  - [`arora`](../crates/arora/readme.md): the opinionated runtime — the
    step loop (`Runtime`), the launcher, and the headless device runner;
  - [`arora-hal`](../crates/arora-hal): the `Hal` trait, the device boundary;
  - [`arora-bridge`](../crates/arora-bridge): the `Bridge` trait, the remote
    boundary (Semio Studio via the studio-bridge connector);
  - [`arora-behavior`](../crates/arora-behavior): the `Behavior` trait ticked
    by the runtime each step;
  - [`arora-simple-data-store`](../crates/arora-simple-data-store): the
    reference `DataStore`, plus the `NamespacedStore` view for mutualizing
    one store across runtimes.

- [`arora-vfs`](../crates/arora-vfs/readme.md),
  provides a virtual file system mostly used in code generation.
  It is potentially useful for WebAssembly modules.

- Arora Module libraries:
  - [`arora-module-core`](../crates/arora-module-authoring/core/readme.md):
    a library to analyze type and module declarations,
    and resolve them for code generation.
  - [`arora-module-cli`](../crates/arora-module-authoring/cli/readme.md):
    a library to generate code from a module description.
    It finds the various code generators locally.
  - [`arora-module-rust`](../crates/arora-module-authoring/rust/readme.md):
    the Rust code generator for modules.
    Also works as a library.
  - [`arora-module-cpp`](../crates/arora-module-authoring/cpp/readme.md):
    the C++ code generator for modules.
    Only works as a executable.

- Modules:
  - [`test-cpp`](../modules/test-cpp/readme.md):
    a module to test the C++ bindings.
  - [`test-cpp-2`](../modules/test-cpp-2/readme.md):
    a module to test the C++ that depends on
    [`test-cpp`](../modules/test-cpp/readme.md)
    and [`arora-behavior-tree-types-yaml`](../crates/arora-behavior-tree-types-yaml/readme.md).
  - [Rust WASM test module](../modules/test-rust-wasm/readme.md):
    a module to test the Rust bindings, that depends on
    [`arora-behavior-tree-types`](../crates/arora-behavior-tree-types/readme.md).
  - [`test-behavior-tree-nodes`](../modules/test-behavior-tree-nodes):
    behavior-tree node functions as a module, exercising the module path in
    tests (the basic control nodes themselves are native in
    [`arora-behavior-tree`](../crates/arora-behavior-tree/readme.md)).
  - [NAO](../modules/nao): a tentative module for NAO support.
  - [Polly](../modules/polly): a module providing nodes for AWS Polly TTS.

- Behavior Tree:
  - [Types](../crates/arora-behavior-tree-types/readme.md):
    basic types used in behavior trees.
  - [Types YAML](../crates/arora-behavior-tree-types-yaml/readme.md):
    the same types serialized as YAML.
  - [Behavior Tree](../crates/arora-behavior-tree/readme.md):
    the Arora-specific library to run behavior trees.
