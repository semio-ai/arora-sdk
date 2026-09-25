# Agent Guide for Semio Arora Engine

## Essential Reading

**Start here:**
1. [`readme.md`](readme.md) — Project overview and high-level concepts
2. [`docs/architecture.md`](docs/architecture.md) — Detailed repo layout and component interaction

These docs explain the runtime, module system, build orchestration, and cross-compilation strategy.

## Key Architectural Concepts

### Where a Module's Interface Lives

A module's interface — its id, functions, parameters and their ids — has exactly
one source, and which one tells you how the module builds:

- **A Rust declaration** (`modules/polly`, `modules/test-rust-wasm`): macros
  from [`arora-module`](crates/arora-module/readme.md) on the Rust module and
  its functions. No `module.yaml`, no `build.rs`, no generated sources; the
  header, the host registration and the guest entry points come out of macro
  expansion. Edit the Rust code.
- **A `module.yaml`** (C++ modules, and Rust modules that import other modules'
  functions, which the macros do not declare): the generator pipeline below.
  A `module.yaml` beside `Cargo.toml` means the YAML is the source of truth.

### Code Generation Pipeline

For `module.yaml` modules, and for crates that generate types (`arora-behavior-tree`):

1. **Module definitions** (`module.yaml`) define the contract: types, functions, imports, exports
2. **Build scripts** (`build.rs`) invoke code generators (`arora-module-cli`, `arora-module-rust`, `arora-module-cpp`)
3. **Generated sources** land in `src/arora_generated/` and are **regenerated on every build**
4. **Manual edits to generated files are lost** — always edit the source `module.yaml` instead

#### Example: Adding a Function to a Module

When adding a function that depends on another module (see
`modules/test-cpp-2/module.yaml`):

```yaml
# modules/my-module/module.yaml
exports:
  - type: function
    id: <new-uuid>
    name: my_function
    parameters: [...]
    ret: [...]

imports:
  - type: function
    module: <dependency-module-uuid>
    id: <dependency-function-uuid>
    name: dependency_function
    parameters: [...]
    ret: [...]

dependencies:
  - dependency-module-name
```

The build script will:
- Read `module.yaml`
- Generate Rust bindings in `src/arora_generated/`
- Generate a `mod.rs` that exposes imported modules
- Generate wrapper functions for cross-module calls

**Do not** manually edit `src/arora_generated/mod.rs` — it will be overwritten.

### Build Orchestration

This workspace uses one **unstable Cargo feature**:
- `-Z bindeps` (artifact dependencies) — enabled in `.cargo/config.toml`
  (`[unstable] bindeps = true`) for host tools and cross-target
  staticlibs/cdylibs.

`per-package-target` is **not** used — no module sets `forced-target`, and the
leftover `cargo-features = ["per-package-target"]` opt-ins were removed from the
module manifests. See `docs/design_decisions.md`.

**Consequence:** Requires nightly Rust (pinned in `rust-toolchain.toml`).

Key build concepts:
- `cargo build` (the default members) is the entry point; `--workspace` also
  builds the opt-in NAO cross-compile, which needs its own toolchain
- Cross-compilation happens automatically via artifact dependencies
- C++ modules use CMake, but invoked from Rust `build.rs`
- Legacy wasm modules target `wasm32-wasip1`; component-model modules
  target `wasm32-wasip2` against `wit/arora-module.wit`
- Browser engine uses `wasm32-unknown-unknown`

## Common Pitfalls and Solutions

### Issue: Test Failures Due to Missing Function in Index

**Symptom:**
```
Error: internal error: function <uuid> is missing from index
```

**Root cause:** The function is referenced in behavior tree code but not exported by any loaded module.

**Solution:**
1. Identify which module should export the function (check the UUID against module definitions)
2. Export it from that module:
   - Rust declaration: add the function to the `#[module]` with `#[export(id = "<uuid>")]`
   - `module.yaml`: add it to `exports`, and if it wraps another module's
     function, to `imports` and the dependency to `dependencies`; then
     `cargo clean -p <module-name>` so the build script regenerates bindings
3. Implement the function in the module's `src/lib.rs`
4. Make sure the test loads that module

### Issue: Registry Error During Build

**Symptom:**
```
Error: no such record "<uuid>"
```

**Root cause:** The build script needs the imported module's definition in the registry, but it's not available.

**Solution:** The module's `build.rs` needs to add the dependency module to its registry before analyzing the module definition. Check `modules/test-cpp-2/build.rs` for an example of importing from another module.

### Issue: Compilation Errors After Editing Generated Files

**Symptom:** Manual edits to `src/arora_generated/*.rs` files disappear or cause build errors.

**Solution:** Never edit generated files directly. Instead:
1. Edit the source `module.yaml`
2. Update `build.rs` if needed
3. Run `cargo clean -p <module-name>` to force regeneration
4. Build again

## Development Workflow

### Running Tests

```bash
# All tests
cargo test

# Specific crate
cargo test -p arora-behavior-tree

# Specific test
cargo test -p arora-behavior-tree graph::tests::groot_lowers_to_graph_and_runs

# With output
cargo test -- --nocapture
```

### Building Modules

```bash
# Default members (everything but the opt-in NAO cross-compile)
cargo build

# Specific module
cargo build -p test-rust-wasm

# A module.yaml module after changing its YAML: clean to force regeneration
cargo clean -p test-behavior-tree-nodes && cargo build -p test-behavior-tree-nodes
```

### Debugging Build Scripts

Build script output goes to stderr. To see it:
```bash
cargo build -p <module-name> 2>&1 | grep -v "Compiling"
```

Or set verbosity:
```bash
cargo build -vv -p <module-name>
```

### Cross-Compilation

The workspace handles cross-compilation via artifact dependencies:
- Rust wasm guests build for the **host** by default; their `wasm32-wasip1`
  flavour is forced on demand by the behavior-tree and integration-test
  crates' artifact dependencies, so `cargo test` builds them automatically.
- Host tools (CLI, code generators) build for host.
- Build scripts receive artifact paths via env vars. **Mind the names:**
  `CARGO_BIN_FILE_<DEP>` works for bins, but for dash-named staticlib/cdylib
  crates cargo only sets `CARGO_<KIND>_FILE_<DEP>_<lib>` (dashes→underscores)
  and `CARGO_<KIND>_DIR_<DEP>` — not the bare `CARGO_<KIND>_FILE_<DEP>`.
  See `docs/design_decisions.md`.

## Module Development

### Creating a New Module

**In Rust**, the crate carries the interface — no `module.yaml`, no `build.rs`:

1. Create directory under `modules/<name>/`
2. Depend on [`arora-module`](crates/arora-module/readme.md)
3. Put `#[arora_module::module(id = "…", …)]` on the Rust module and
   `#[export(id = "…")]` / `#[param(id = "…")]` on its functions
4. Build and test; a host registers it with `HostModule::of::<M>()`

See [`modules/test-rust-wasm/`](modules/test-rust-wasm/) (wasm guest) or
[`modules/polly/`](modules/polly/) (native) for working examples.

**In another language** (C++), the interface is a `module.yaml` and the
`arora-module-*` generators produce the bindings: write the YAML, a `build.rs`
calling `arora-module-core`, then implement the functions. See
[`modules/test-cpp/`](modules/test-cpp/).

### Module Interface Contract

All modules communicate via:
- **Structures** for call arguments and returns (UUID-identified)
- **Buffers** for serialization (see `arora-buffers`)
- **Dispatch functions** for cross-module calls:
  - `arora_dispatch(module_id, method_id, arg)` — direct call
  - `arora_dispatch_indirect(callable_id)` — anonymous callable

See [`arora-types`](crates/arora-types/readme.md) for the runtime contract.

## Testing Strategy

- **Unit tests:** In `src/` files using `#[cfg(test)]`
- **Integration tests:** In `tests/` directory (see `arora-integration-tests`)
- **Module tests:** Often in `crates/arora-behavior-tree/src/tests.rs`

Behavior tree tests often need to:
1. Set up an engine with specific modules loaded
2. Build a behavior tree referencing those modules
3. Tick the tree and verify results

Helper functions (test-local, e.g. `modules/polly/tests/`):
- `setup_engine_with_modules(&vec!["module-name"])`
- `add_module_functions_to_index()` — populates function lookup

## Crate Purposes

Quick reference:

| Crate | Purpose |
|-------|---------|
| `arora-engine` | Core engine (host + browser executors) |
| `arora` | Opinionated device runtime (engine + native BT nodes + step loop) |
| `arora-cli` | Command-line interface |
| `arora-web` | Browser wasm bindings |
| `arora-buffers` | Serialization primitives |
| `arora-registry` | Type/module registry (local + remote) |
| `arora-module` | Declare a module in Rust (facade over the macros + `AroraModule`) |
| `arora-module-macros` | Proc macros behind `arora-module` (use through the facade) |
| `arora-module-core` | Module analysis and resolution |
| `arora-module-cli` | Code generator CLI |
| `arora-module-rust` | Rust code generation |
| `arora-module-cpp` | C++ code generation |
| `arora-behavior-tree` | Behavior tree runtime |
| `arora-behavior-tree-types` | BT primitive types |

## Tips for AI Agents

1. **Always read the README and docs first** — they contain critical context about build orchestration and cross-compilation
   
2. **Know where the interface lives** — a Rust declaration or a `module.yaml`, never both; edit that source, not what is derived from it
   
3. **Use `cargo clean -p <module>` after changing a `module.yaml`** — it forces regeneration
   
4. **Check UUIDs carefully** — function/type mismatches often come down to UUID confusion between similar concepts
   
5. **Test module dependencies in isolation** — if a test fails with "function missing from index", verify the module actually exports it and the test loads that module
   
6. **Grep for examples** — this codebase has patterns repeated across modules; find a working example and adapt it
   
7. **Watch out for generated files** — if you see `src/arora_generated/`, don't edit it directly
   
8. **Build scripts are key for `module.yaml` modules** — if something doesn't generate correctly, the bug is likely in `build.rs`, not the source files
   
9. **Imports vs Dependencies** — `module.yaml` modules need BOTH:
   - `imports:` section for functions you'll call
   - `dependencies:` section for modules to link against
   
10. **Registry matters** — the build-time registry needs to know about imported modules; check `build.rs` adds them before `analyze_module_from_path()`

## When Things Break

Diagnostic checklist:

- [ ] Rust declaration: do the `#[export]` / `#[param]` ids match the ids callers use?
- [ ] Is `module.yaml` syntactically correct?
- [ ] Do all referenced UUIDs exist in the registry?
- [ ] Are imported modules listed in both `imports:` and `dependencies:`?
- [ ] Does `build.rs` add dependency modules to the registry?
- [ ] Did you clean the package after editing `module.yaml`?
- [ ] Are you editing generated files instead of source files?
- [ ] Do test helper functions load all necessary modules?
- [ ] Are function parameter/return types correctly specified?

## Related Repositories

- [arora-types](crates/arora-types/readme.md) — Core type definitions and runtime contract
- [semio-record](https://github.com/semio-ai/semio-record) — Record system for types and modules
- [semio-client](https://github.com/semio-ai/semio-client) — Client for Semio database

## Maintenance Notes

This AGENTS.md file should be updated when:
- Build system changes (new unstable features, toolchain requirements)
- Code generation pipeline evolves
- New common pitfalls are discovered
- Module development patterns change
- Cross-compilation strategy shifts

Keep it focused on what AI agents need to know to work effectively on the codebase.
