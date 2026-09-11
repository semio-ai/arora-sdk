# Open questions

Design choices the study cannot make on its own. Each states what is
established, the options, and a recommendation. A **Decided** line records
the choice once made; the mechanism pages follow it.

## Q1 — What does a parameter's Rust type mean? {#q1}

Today's generated shim declares every parameter as `Option<T>`: an absent
argument arrives as `None`, and the author unwraps. The declaration macro reads
the Rust type as the declared type: `text: String` declares `str`, and an
absent argument fails the call by name (tested on polly). `Option<T>` would
then declare an optional parameter — a `TypeRef::Option` exists in the header
vocabulary, but the record vocabulary (`FrozenTy`) has no optional form.

- **A.** Rust type = declared type; absent non-`Option` argument = call error.
  Optional parameters wait on the record vocabulary.
- **B.** Keep the generated convention: every parameter `Option<T>`, absent =
  `None`.

Recommendation: **A**. It is what a Rust signature says, it is what the host
shim already does, and a missing argument reported by name is a better failure
than an `unwrap` in the author's code.

**Decided: A.**

## Q2 — What must a parameter or return type implement? {#q2}

The macro needs, for a type `T`: its arora id and definition (`AroraType`),
and a way across the `Value` boundary. Established: `value_serde`'s seeded walk
handles structs but refuses enumerations; explicit `Into<Value>` /
`TryFrom<Value>` impls handle both, and are what `Status` and the ARORA-55
generated structs carry today.

- **A.** Require `AroraType + Into<Value> + TryFrom<Value>`; authors write or
  generate the conversions (status quo for `Status`; the codegen for structs).
- **B.** Require `AroraType + Serialize + Deserialize` and go through the
  seeded serde walk; extend the walk to enumerations.
- **C.** Extend `#[derive(AroraType)]` to also emit `Into<Value>` /
  `TryFrom<Value>` from the pinned ids, so one derive gives schema and
  conversions; the macro requires **A**'s bounds and the derive satisfies them.

Recommendation: **C**. The derive already knows every id the conversions need;
today they are produced twice (by the derive for the schema, by the codegen or
by hand for the conversions) and must agree.

**Decided: C.**

## Q3 — What does the declaration expose, and under what names? {#q3}

The prototype appends free items to the module: `ids::{MODULE, <fn>::{FUNCTION,
<PARAM>}}`, `header()`, `host_module()`.

- **A.** Free items in the module, as prototyped.
- **B.** A trait, mirroring `AroraType`: `impl AroraModule for <marker>` with
  `fn id()`, `fn header()`, `fn host_module()`, so a device builder can take
  any declared module generically (`with_module::<M>()`).
- **C.** Both: free items for ergonomics, a trait impl for generic code.

Recommendation: **A now**, keep **B** in view; nothing generic over modules
exists yet to justify the trait.

**Decided: C** — free items and a trait. The aggregate emits a marker type
(`polly::Module`) implementing `AroraModule` (`id()`, `header()`,
`host_functions()`), the twin of `AroraType`; the host side becomes a generic
`host_module::<M>()`.

## Q4 — Where does `host_module()` live? {#q4}

Established: emitting it unconditionally makes the declaring crate depend on
`arora-engine` (without executors), and that crate still builds for
`wasm32-wasip1` (polly). Unknown: whether that stays true for wasm targets
other than wasip1 and for every module crate.

- **A.** Always emitted; the declaring crate depends on `arora-engine`
  (`default-features = false`).
- **B.** Opt-in: `#[module(…, host)]` emits it; a pure guest crate does not
  need the dependency.
- **C.** A separate macro invoked from the host crate:
  `arora::host_module!(polly)` — the module crate stays free of `arora-engine`.

Recommendation: **B**. A guest crate should not carry the engine's types, and
the host side is a property of the build, not of the interface.

**Decided: C** — the host crate assembles the `HostModule`; the module crate
never depends on `arora-engine`. Consequence for the mechanism: the module
crate exposes its host closures over `arora-types` only (`Call ->
Result<CallResult, CallError>` needs nothing from the engine), and the host
side — a function in `arora-engine`, or a thin macro — folds them into a
`ModuleBuilder`.

## Q5 — Modules too large to inline {#q5}

Established: an attribute macro sees only an inline `mod name { … }`.

- **A.** Inline modules only; a large module is a large inline module.
- **B.** Explicit listing: `#[module(…, exports = [say, hello_world])]` on
  `mod name;`, with `#[export]` still on each function (checked against the
  list).
- **C.** A crate-level `arora::module! { id …; exports { say, hello_world } }`
  that names functions from anywhere in the crate.

Recommendation: **A** as the primary form, since the module *is* the
declaration; **B** as the escape hatch, because it keeps one declaration site.

**Decided: A + B.** Consequence for the mechanism: with `mod name;` the module
macro sees neither signatures nor parameter ids, so `#[export]` must itself be
a macro that emits each function's declaration beside it; `#[module]`
aggregates those — by scanning an inline module, or by the explicit list.

## Q6 — Where does a user type's record version come from? {#q6}

A frozen signature pins each user type to a version (`Status@1.0.0` in every
hand-built signature today). `AroraType` carries no version; the prototype
pins `1.0.0`.

- **A.** `AroraType` gains `fn arora_type_version() -> Version` (default
  `1.0.0`), settable through `#[arora(version = "…")]`.
- **B.** The version is per use: `#[param(id = "…", version = "…")]`.
- **C.** Frozen signatures built from a declaration always pin `1.0.0`; the
  version is a store concern applied when the record is published.

Recommendation: **A**. A type's version is the type's, and one place to state
it keeps every signature that references it consistent.

**Decided: A.** `#[arora(version = "…")]` on the type, `1.0.0` when absent;
the frozen signatures read it from the type.

## Q7 — Does the declaration also emit the guest exports? {#q7}

The vision says yes: the `#[no_mangle] arora_function_<uuid>` shims and the
buffer codec are "the boilerplate for module authors". Nothing tested yet
([M4](README.md#mechanisms)).

- **A.** Yes, from the same declaration, behind the `wasm32` target cfg —
  `arora-module-rust` is then only for non-declared (YAML-first) Rust modules.
- **B.** No: the declaration writes `module.yaml`, and `arora-module-rust`
  keeps generating the guest side from it.

Recommendation: **A**, to be validated by the test-rust-wasm case: it is the
half of the vision that removes `build.rs` from a Rust guest.

**Decided: A.**

## Q8 — Interface → Rust: what does the reverse macro consume? {#q8}

A caller wants stubs for a module it does not implement. The header form with
resolved ids (`low`, what the generator writes into `arora_generated/module.yaml`)
carries everything except type *definitions*; the source form (`high`,
`module.yaml` as authored) names types by path (`behavior_tree.Status`) and
needs a registry to resolve them.

- **A.** Consume the `low` header (`module_from_header!("…/module.yaml")`),
  producing ids and stubs whose user types the caller supplies (by
  `AroraType`).
- **B.** Consume the `high` form, resolving names through records shipped
  alongside (`types/*.yaml`), producing the types too.
- **C.** Consume a Rust declaration from another crate: the declaring crate's
  `header()` is the interface; the caller depends on the crate (or a thin
  interface crate) — no YAML in the loop.

Recommendation: **C** for Rust-to-Rust, **A** for a module whose author is not
Rust. **B** re-implements the generator's registry resolution inside a macro.

**Decided: C + A.**

## Q9 — The client stub's `module_id` parameter {#q9}

`polly::client::say(&mut bridge, polly::ids::MODULE, text)`: the stub takes
the module id because `#[export]` expands before the aggregate names the
module, so the per-function code cannot know it.

- **A.** Keep it: a stub is usable against a module registered under another
  id (a test double), and the aggregate could add an id-bound wrapper later.
- **B.** The aggregate emits the `client` functions itself, binding
  `ids::MODULE` — it needs the signatures, which means `#[export]` also leaves
  a `macro_rules!` per function the aggregate can invoke.
- **C.** `#[export(id = …, module = "<uuid>")]` repeats the module id on every
  function.

Recommendation: **A** for now; **B** once the ergonomics matter.

**Decided: B.** `#[export]` leaves a textually scoped `macro_rules!` per
function; `declare_module!` invokes each with `ids::MODULE` inside `client`,
so a consumer writes `polly::client::say(&mut bridge, text)`.

## Q10 — What `executor` a host-only module's header names {#q10}

A header names the executor that loads its artifact. The vizij skills, the
interpreter module, any `host_module::<M>()` registration have no artifact
and no executor; the prototype writes `"host"`. The runtime's
`Engine::load_module` would refuse it (`ExecutorNotFound`), which is right —
such a module is registered, not loaded.

- **A.** `"host"` as a reserved executor name meaning "registered in-process".
- **B.** `executor` becomes optional in the header (`Option<Executor>`), absent
  for host-only modules.
- **C.** A host-only module declares the executor its *implementation* would
  ship under if built (`native`, `wasm`), so the header is complete the day an
  artifact exists.

Recommendation: **A**: it states what is true, and a store record or a
`DescribeMethods` consumer can tell in-process modules apart.

**Decided: B, by this rationale.** The executor is not knowable at the
declaration: only the step that compiles and exports the module — building
the artifact — can tell whether it is native or wasm. A consumer importing the
interface neither knows nor cares. It matters at **load**, and there are two
ways to load: linking directly against the Rust symbols (the host case), where
no header is involved; or providing a description plus a binary, where the
description **must** name the executor — `"host"` is not an executor, and
`None` is not acceptable there. So `Header::executor` becomes
`Option<Executor>`: `None` from a declaration, `Some` set at export, required
by `Engine::load_module`. The prototype cannot change the SDK type; it models
the same split with `header(executor)` — the exporter supplies it — and a
host-only module's interface is exported as its `record(parent)`, which has
no executor field at all.
