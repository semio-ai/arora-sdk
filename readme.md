# Arora

**Arora is a runtime and toolkit for defining typed functions as portable modules and calling them across languages and contexts** — natively, in WebAssembly, or in the browser.

Write a module in Rust or C++, declare its typed interface once, and any Arora host can load it and call its functions. Orchestrate those calls however you like: by hand, from a CLI, from the web, or as a behavior tree.

> **In one breath:** [`arora-types`](#the-map) is the shared type system, [`arora-engine`](#the-map) loads and runs modules, [`arora-behavior-tree`](#the-map) orchestrates calls as trees, and [`arora`](#the-map) is the batteries-included wrapper. → [Getting started](#getting-started).

## The map

Arora is a single Cargo workspace of focused crates, layered from a neutral core up to opinionated wrappers. Each crate is published independently to crates.io.

| Crate | What it is |
|---|---|
| **`arora-types`** | The basic typing system shared by everything in Arora: module headers, type/record references, values, and the `CallBridge` calling interface. Depend on this alone if you only need the vocabulary. |
| **`arora-module-authoring`** | Produce and import modules — the codegen core, the per-language supports (Rust, C++, …), and the authoring CLI. (Several crates today; they belong together.) |
| **`arora-registry`** | Resolve modules, their versions and dependencies — locally, or, optionally, against a remote store. |
| **`arora-engine`** | The generic runtime: load modules and call arbitrary typed functions in multiple execution contexts (native via `wasmtime`, in-browser via the platform `WebAssembly`). Implements the `CallBridge`. |
| **`arora-behavior-tree`** | A standalone crate that uses the `CallBridge` (typically implemented by the engine) to call any function from any module — orchestrated as a behavior tree. Usable **without** the authoring crates; the module-backed node support is feature-gated (opt-out). |
| **`arora`** | The opinionated wrapper: the engine pre-wired with behavior trees as the entry point, backed by Semio's services. Start here if you want "Arora, batteries included". |
| **`arora-cli`** | The command line for Arora — drive any engine configuration, with Semio's backend integrated, promoting the opinionated defaults. |
| **`arora-web`** | The opinionated Arora, packaged for the browser as an NPM package. |

### How they fit together

```
arora-types ───────────────────── the vocabulary everything shares
   ├─ arora-module-authoring ───── build & import modules
   ├─ arora-registry ──────────── resolve modules + dependencies
   └─ arora-engine ────────────── load & run modules  (provides the CallBridge)
         ├─ arora-behavior-tree ── orchestrate calls as trees (consumes the CallBridge)
         └─ arora ──────────────── opinionated: engine + behavior tree + Semio backend
               ├─ arora-cli ────── command line
               └─ arora-web ────── browser / NPM package
```

The key seam is the **`CallBridge`** (in `arora-types`): the engine *implements* it (it can call any loaded module function), and the behavior tree *consumes* it (it calls functions without knowing or caring who provides them). That decoupling is why `arora-behavior-tree` can be used on its own, against any `CallBridge`.

## Getting started

```sh
# Build the whole workspace
cargo build

# Run the test suite (builds the example wasm/C++ modules and calls them)
cargo test
```

See [`docs/building.md`](docs/building.md) for prerequisites, the browser/wasm target, and build flags.

- **Embedding the runtime?** Look at `arora-engine` — `EngineBuilder`, the executors, and `load_module_from_parts`.
- **Writing a module?** Look at `arora-module-authoring` — declare a `module.yaml`, generate bindings, build to wasm.
- **Behavior trees?** Look at `arora-behavior-tree`.
- **Just want it to work?** Reach for `arora`, `arora-cli`, or `arora-web`.

Each crate carries its own `readme.md`. Going deeper:

- [`docs/overview.md`](docs/overview.md) — engine, modules, records, behavior trees, full layout
- [`docs/building.md`](docs/building.md) — prerequisites, build, testing, browser target, flags
- [`docs/architecture.md`](docs/architecture.md) — top-down architecture tour
- [`docs/design_decisions.md`](docs/design_decisions.md) — the *why* behind the build setup
- [`docs/dispatch.md`](docs/dispatch.md) — how cross-module calls (the `CallBridge` / `arora_dispatch`) work

## Where this came from

Arora was split across several repositories while we sorted out what would be
open source. It now lives as **one public workspace** (`arora-sdk`); the only
external dependency is Semio's own backend services (the hosted record store),
which the opinionated `arora` / `arora-cli` / `arora-web` layer integrates and
which everything below `arora-engine` can be built without.
