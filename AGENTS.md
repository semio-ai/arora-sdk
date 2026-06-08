# Agent Guide for Semio Arora Engine

## Essential Reading

**Start here:**
1. [`readme.md`](readme.md) — Project overview and high-level concepts
2. [`docs/architecture.md`](docs/architecture.md) — Detailed repo layout and component interaction

These docs explain the runtime, module system, build orchestration, and cross-compilation strategy.

## Key Architectural Concepts

### Code Generation Pipeline

**Critical:** Many parts of this repository generate code for other parts. Understanding this pipeline is essential:

1. **Module definitions** (`module.yaml`) define the contract: types, functions, imports, exports
2. **Build scripts** (`build.rs`) invoke code generators (`arora-module-cli`, `arora-module-rust`, `arora-module-cpp`)
3. **Generated sources** land in `src/arora_generated/` and are **regenerated on every build**
4. **Manual edits to generated files are lost** — always edit the source `module.yaml` instead

#### Example: Adding a Function to a Module

When adding a function that depends on another module:

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

`per-package-target` is **not** used (no module sets `forced-target`); the
`cargo-features = ["per-package-target"]` lines in `modules/test-cpp*/Cargo.toml`
are vestigial. See `docs/design_decisions.md`.

**Consequence:** Requires nightly Rust (pinned in `rust-toolchain.toml`).

Key build concepts:
- `cargo build --workspace` is the entry point
- Cross-compilation happens automatically via artifact dependencies
- C++ modules use CMake, but invoked from Rust `build.rs`
- Wasm modules target `wasm32-wasip1`
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
2. Add the function to that module's `module.yaml` exports section
3. If the function wraps another module's function, add it to `imports` as well
4. Add the dependency module to the `dependencies` list
5. Implement the function in the module's `src/lib.rs`
6. Clean build the module: `cargo clean -p <module-name>`
7. The build script will regenerate bindings automatically

**Real example:** The `cos` function (UUID `104b9710-5d43-4a93-944c-d64bddb30ef8`) needed to be:
- Exported by `behavior-tree-nodes` module
- Implemented as a wrapper calling `test-rust-wasm::cos`
- Imported from `test-rust-wasm` (UUID `c13757cb-2311-4c93-abcc-cb12d6cbb859`)
- Added `test-rust-wasm` to dependencies list

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
cargo test --lib schema_groot::tests::tree_node_from_groot

# With output
cargo test -- --nocapture
```

### Building Modules

```bash
# Workspace build (includes all modules)
cargo build --workspace

# Specific module (will trigger build.rs and code generation)
cargo build -p behavior-tree-nodes

# Clean a specific module (useful after changing module.yaml)
cargo clean -p behavior-tree-nodes && cargo build -p behavior-tree-nodes
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

1. Create directory under `modules/<name>/`
2. Write `module.yaml` defining types, functions, imports
3. Write `build.rs` using `arora-module-core` to analyze and generate code
4. Write `Cargo.toml` with necessary artifact dependencies
5. Implement functions in `src/lib.rs`
6. Build and test

See [`modules/test-rust-wasm/`](modules/test-rust-wasm/) or [`modules/behavior-tree-nodes/`](modules/behavior-tree-nodes/) for working examples.

### Module Interface Contract

All modules communicate via:
- **Structures** for call arguments and returns (UUID-identified)
- **Buffers** for serialization (see `arora-buffers`)
- **Dispatch functions** for cross-module calls:
  - `arora_dispatch(module_id, method_id, arg)` — direct call
  - `arora_dispatch_indirect(callable_id)` — anonymous callable

See [`arora-types`](https://github.com/semio-ai/arora-types) for the runtime contract.

## Testing Strategy

- **Unit tests:** In `src/` files using `#[cfg(test)]`
- **Integration tests:** In `tests/` directory (see `arora-integration-tests`)
- **Module tests:** Often in `crates/arora-behavior-tree/src/tests.rs`

Behavior tree tests often need to:
1. Set up an engine with specific modules loaded
2. Build a behavior tree referencing those modules
3. Tick the tree and verify results

Helper functions:
- `setup_engine_with_modules(&vec!["module-name"])`
- `read_header_to_index()` — loads module definitions into index
- `add_module_functions_to_index()` — populates function lookup

## Crate Purposes

Quick reference:

| Crate | Purpose |
|-------|---------|
| `arora` | Core engine (host + browser) |
| `arora-cli` | Command-line interface |
| `arora-web` | Browser wasm bindings |
| `arora-buffers` | Serialization primitives |
| `arora-registry` | Type/module registry (local + remote) |
| `arora-module-core` | Module analysis and resolution |
| `arora-module-cli` | Code generator CLI |
| `arora-module-rust` | Rust code generation |
| `arora-module-cpp` | C++ code generation |
| `arora-behavior-tree` | Behavior tree runtime |
| `arora-behavior-tree-types` | BT primitive types |

## Tips for AI Agents

1. **Always read the README and docs first** — they contain critical context about build orchestration and cross-compilation
   
2. **Understand the code generation flow** — many compilation errors stem from not understanding that `module.yaml` is the source of truth
   
3. **Use `cargo clean -p <module>` liberally** — when changing `module.yaml`, force regeneration
   
4. **Check UUIDs carefully** — function/type mismatches often come down to UUID confusion between similar concepts
   
5. **Test module dependencies in isolation** — if a test fails with "function missing from index", verify the module actually exports it and the test loads that module
   
6. **Grep for examples** — this codebase has patterns repeated across modules; find a working example and adapt it
   
7. **Watch out for generated files** — if you see `src/arora_generated/`, don't edit it directly
   
8. **Build scripts are key** — if something doesn't generate correctly, the bug is likely in `build.rs`, not the source files
   
9. **Imports vs Dependencies** — modules need BOTH:
   - `imports:` section for functions you'll call
   - `dependencies:` section for modules to link against
   
10. **Registry matters** — the build-time registry needs to know about imported modules; check `build.rs` adds them before `analyze_module_from_path()`

## When Things Break

Diagnostic checklist:

- [ ] Is `module.yaml` syntactically correct?
- [ ] Do all referenced UUIDs exist in the registry?
- [ ] Are imported modules listed in both `imports:` and `dependencies:`?
- [ ] Does `build.rs` add dependency modules to the registry?
- [ ] Did you clean the package after editing `module.yaml`?
- [ ] Are you editing generated files instead of source files?
- [ ] Do test helper functions load all necessary modules?
- [ ] Are function parameter/return types correctly specified?

## Related Repositories

- [arora-types](https://github.com/semio-ai/arora-types) — Core type definitions and runtime contract
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
