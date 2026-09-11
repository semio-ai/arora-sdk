# Declaring Arora modules in Rust

A study of one change: **a Rust crate can carry an Arora module's interface**,
declared with macros on the Rust module and its functions — the way
`#[derive(AroraType)]` already lets a Rust type carry its own schema — instead
of a `module.yaml` transcribed by hand into every place that needs its ids.

Every claim here is either exercised by a test in [`prototype/`](prototype/)
(a standalone workspace: `prototype/test.sh` builds the guest artifact and
runs the 37 tests) or points at the code that establishes it. The module crates under `prototype/cases`
depend on `arora-types` only; `cases/host` is the one crate that depends on
`arora-engine`, and it is where every declared module is dispatched. Claims not yet exercised are marked **pending**.

## The vision

**A module interface has several representations, and none of them is the
module.** `module.yaml` is one. A record in a store — semio-db today, a
representation served by semio-studio tomorrow — is another, and it differs:
records are versioned and frozen, headers are not. A Rust crate can be a third.
What must hold across them is the interface itself: the module's id, its
functions, their parameters, each pinned by id.

**Generating code from an interface stays.** Taking an interface and producing
client stubs for callers, or the boilerplate an author fills in, is convenient
and it is how other languages are reached at all (the C++ generator). The
`arora-module-*` crates and their command-line tools keep doing that.

**But when a module is written in Rust and consumed in Rust, those tools are a
burden.** A `build.rs`, a bindep on the generator, generated sources committed
into the crate, and — on the consuming side — the ids copied by hand because
nothing exports them. Host-side modules are only the most visible case; a
module written in Rust for a wasm guest pays the same. If the crate carried its
interface, the build would not have to hop through a generator to learn it.

**Rust's macros absorb exactly this kind of redundancy.** `#[derive(AroraType)]`
turns a Rust type into the source of truth for its schema: the type's id and
field ids are pinned in attributes, and the schema is produced from the
definition — exportable as YAML or anything else. The reverse direction exists
too: a type definition from elsewhere (ROS 2 messages) becomes a Rust type that
imports directly.

**The same, for modules.** Declare a module for Arora by putting a macro on a
Rust module and one on each exported function; get back the module's Rust
symbols — its ids, its header, its host-side registration, its guest exports.
And the reverse: take a module interface — a `module.yaml`, or one written
inline in Rust — and get the Rust symbols to call it.

## Features

What a Rust author or consumer gets once this exists:

- **F1 — Declare a module in Rust.** The interface lives in the crate that
  implements it; no `module.yaml` to keep in step, no generated sources.
  *(M1, every case.)*
- **F2 — Export the interface.** The declaration produces the header any other
  representation is written from: a `module.yaml`, a store record. *(M6: the
  vizij skills export a record they never had; polly's record round-trips.)*
- **F3 — Register host-side from the declaration.** Ids, signatures and the
  argument marshalling of a `HostModule` come from the declaration, not from a
  hand-written mirror. *(M1, `host_module::<M>()`.)*
- **F4 — Build the guest from the declaration.** The wasm exports come from the
  same declaration, with no `build.rs`. *(M4.)*
- **F5 — Consume a module from its interface.** A caller gets typed Rust stubs
  for a module it does not implement, from that module's interface. *(M5.)*
- **F6 — One declaration, one set of ids.** Wherever a module's ids appear —
  header, host, tests, another language's tables — they derive from the
  declaration. *(`ids::…` in every case; the TypeScript tables become a
  target for a generator over `header()`.)*

## Mechanisms

How the features are obtained, and what is established about each:

| | Mechanism | Status |
|---|---|---|
| M1 | `#[export]` / `#[param]` on functions; `declare_module!` (or `#[module]` on an inline module) aggregating them into `ids`, `header()`, `host_functions()`; `host_module!` on the host side — [mechanism page](mechanism-declaration-macro.md) | **tested**, both declaration forms ([polly](case-polly.md)) |
| M2 | Rust type ↔ arora type mapping shared with `#[derive(AroraType)]`: primitives by table, `Vec<T>` as arrays, `Value` as KeyValue, anything else through `AroraType::arora_type_id()`; both header and record vocabularies emitted from it | **tested** for primitives, enums, structs, arrays of both ([animation](case-animation-arora-81.md)); `Option`/maps rejected (no record form) |
| M3 | Parameter and return values cross the `Call` boundary through `Into<Value>` / `TryFrom<Value>`, derived from the pinned ids (`#[derive(AroraValue)]`, to fold into `AroraType`), which also carries the type's record version — [Q2](open-questions.md#q2), [Q6](open-questions.md#q6) | **tested**: wire-identical to the ARORA-55 generated conversions; version pinned in signatures and record dependencies |
| M4 | The guest `#[no_mangle] arora_function_<id>` exports emitted from the declaration over the `Value` wire codec, replacing `arora-module-rust` for Rust guests — [guest case](case-test-rust-wasm.md) | **tested** through the real `WebAssemblyExecutor` |
| M5 | Interface → Rust: client stubs from the declaring crate (`client::<fn>`), and `module_from_header!` over a resolved header for modules that are not Rust declarations — [imports case](case-imports.md) | **tested**, both sources |
| M6 | The declaration's `header(executor)` is what a `module.yaml` is written from at export (the executor is the exporter's, not the declaration's — [Q10](open-questions.md#q10)); its `record(parent)` is the store's frozen module record | **tested**: header equal to the generator's; record round-trips in the store's wire form with its versioned dependencies |

## Impacts

Every place a module is declared, generated, registered or transcribed today,
and what the mechanisms do to it. The [inventory](inventory.md) lists them all;
each case below takes the code out into the prototype so the replacement is
compiled and tested.

| Case | Today | Study |
|---|---|---|
| [polly](case-polly.md) — a native module with a `module.yaml` and generated sources | `build.rs` + `arora_generated/` | **done**: declared in Rust, header equal to the generator's, host module dispatching by id |
| [animation module — ARORA-81](case-animation-arora-81.md) | five hand-written copies of the ids; positional host arguments | **done**: six exports declared, header equal to the generator's, struct/array boundary crossed, reversed-argument property held |
| [interpreter module](case-interpreter-module.md) — a host-only module with no `module.yaml` | ids and codec by hand | **out of reach**: its payloads hold maps, which neither `AroraType` nor the record vocabulary can express |
| [vizij host modules](case-vizij-host-modules.md) — tts, tts_piper, viseme, gaze | `described_function` with hand-built `Function`s | **done**: `say` and `look_at` declared, signatures equal to the hand-built ones, a header exported where none existed |
| [behavior-tree nodes](case-behavior-tree-nodes.md) — mutable parameters, `children` | generated guest | **done**: mutable parameters written back under their id; `children` in as an array of structures |
| [the wasm guest](case-test-rust-wasm.md) — test-rust-wasm and every other Rust guest | generated guest | **done** on bt-nodes: built for `wasm32-wasip1`, loaded and dispatched by the real executor |
| [imports](case-imports.md) — calling a module | generated client stubs / hand-built `Call`s | **done**: stubs from the declaration and from a bare header |

## Open questions

The design choices that are yours: [open-questions.md](open-questions.md).
