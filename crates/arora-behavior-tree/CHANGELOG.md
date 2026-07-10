# Changelog

All notable changes to `arora-behavior-tree`. The format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow
[Semantic Versioning](https://semver.org/).

## [5.0.1] - 2026-07-10

### Changed

- Refreshed documentation; the crate now ships its CHANGELOG.

## [5.0.0] - 2026-07-10

### Breaking

- The interpreter is a module — generic function modules replace HostFunction

## [4.2.0] - 2026-07-10

### Added

- Predetermined slots bind to the store unless linked (API-consistency PR 6)

## [4.1.0] - 2026-07-10

### Breaking

- 6.0.0 — the runtime's call dispatch is part of its face
- The golden behavior edit — a Call reaches interpreter.apply through the engine (PR 5b)

## [4.0.0] - 2026-07-09

### Breaking

- 4.0.0 — the graph-lowered interpreter is its own major
- Call_bridge, and edition that defers lowering to the tick
- A slot's direction is which Node list holds it
- Lower Groot import onto the shared Graph; arora glue
- Lower the shared Graph model onto the tree
- Shared graph model + GraphDiff + BehaviorInterpreter::apply

## [3.0.0] - 2026-07-09

### Breaking

- 3.0.0 — the empty-ready interpreter is its own major
- Design B — run drains the seams; the device owns them
- Empty-ready BehaviorTreeInterpreter + load

### Fixed

- Typed Value vocabulary + array-of-enum wire conformance (ARORA-55)
- Conform array-of-struct wire layout to serde_uuid (ARORA-55)
- Nested/recursive/dynamic type codegen (ARORA-55)

## [2.0.0] - 2026-07-08

### Breaking

- Rename Behavior trait to BehaviorInterpreter

## [1.0.0] - 2026-07-08

### Breaking

- Golden clock keys (time/dt) in the store; drop ctx.dt

## [0.1.1] - 2026-07-05

### Fixed

- Ship arora-behavior-tree's generated sources in the crate
- Crates.io forbids wildcard version constraints
- Clippy + readme fallout of the semio-record removal

### Changed

- Serde_yaml 0.9 everywhere — one emitter, one parser, formats unchanged

## [0.1.0] - 2026-07-04

### Breaking

- Drop the private semio-record dependency — type records live in arora-types

### Added

- A Behavior the runtime ticks, in an arora-behavior crate (VIZ-33)
- Bind behavior-tree variables to the data store
- Run the opinionated arora runtime on wasm

### Fixed

- Share one cell per variable id
- Keep children when converting a Groot tree
- Use the single in-workspace arora-types everywhere (incl. tests)

### Changed

- Variable cells use a VariableCell abstraction
- Fix module link + note native basic nodes
- Native control nodes; free arora of the BT-nodes module
- Group module-authoring crates under crates/arora-module-authoring/
- Fix relative links after consolidation + arora-engine rename

## [0.0.1] - 2026-06-26

### Added

- Implement cos and add math functions natively
- Support non-Status return_binding on nodes

### Fixed

- Convert string/regex nodes to Groot
- Resolve hand-written lints; allow on generated subtrees
- Use normalized_value() instead of deprecated unescape_value()
- IndexMap-based semio-record and improved WASM building

### Changed

- Bring the behavior-tree crates back into the workspace
- Extract behavior-tree into its own repo; consume it via git
- Drop the extra Box from Groot Node children
- Group engine tests into one to load modules once
- Use artifact dependency env var for behavior-tree-nodes WASM
- Forward WASM artifact dependency path to tests
- Move polly tests from arora-behavior-tree into modules/polly
- Make BehaviorTreeRuntime and ModuleFunction public
- Auto-rebuild behavior-tree-nodes WASM artifact for tests
- Wire cos and add from behavior-tree-nodes
- Replace return_binding with _ret out-parameter
- Type_mismatch now asserts Err instead of should_panic
- Document type mismatch as panic (WASM trap)
- Update READMEs for return_binding, BehaviorTreeRunner, and demo
- Add return_binding tests

