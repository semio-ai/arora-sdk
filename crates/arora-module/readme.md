# arora-module

Write an Arora module in Rust. The crate that implements the module carries
its interface — the module's id, its functions, their parameters, each pinned
by id — and every other form the module takes comes from that one
declaration: the header an engine loads, the record a registry holds, the
functions a host registers, the stubs a caller programs against, and the
entry points of the built artifact.

## Declare a module

```rust
#[arora_module::module(id = "a1a6bb9a-…", name = "polly", version = "0.1.0")]
pub mod polly {
  #[export(id = "e1b4bda7-…")]
  pub fn say(#[param(id = "fb3787f2-…")] text: String) -> Status { … }
}
```

`#[module]` goes on an inline Rust module and finds its `#[export]` functions;
`#[param]` pins each parameter's id. Ids are written in full, never derived
from names, so renaming a function or a parameter keeps its id. A parameter
the function writes through is `&mut T`: the engine returns what it wrote
with the result.

A host linking the crate registers the module whole, and calls it like any
other:

```rust
engine.register_module(polly::ids::MODULE, Box::new(HostModule::of::<polly::Module>()));
let status = polly::client::say(&mut engine, "hello".to_string())?;
```

## What the declaration yields

| | |
|---|---|
| `polly::ids` | the module id (`MODULE`), and per function its id (`say::FUNCTION`) and its parameters' (`say::TEXT`) |
| `polly::header(executor)` | the [header](../arora-engine/readme.md#modules-executors-and-calls) an engine loads the artifact with |
| `polly::record(parent)` | the module's frozen record, under the folder `parent` |
| `polly::type_records()` | the records of the types its functions name, and of the types those name |
| `polly::exports()` | every function, callable — what `HostModule::of::<polly::Module>()` registers |
| `polly::client::say(&mut bridge, …)` | the typed stub, calling through any `CallBridge` |
| `polly::Module` | the marker type implementing `AroraModule`, for code generic over declared modules |
| `polly::function_ids()`, `polly::dispatch_buffer(method, arg)` | the functions the artifact serves, and a call as it receives one |
| `arora_function_<id>` | the entry point a shared library exports per function, for the `native` executor |

**The executor is not the declaration's to name.** Only the step that builds
the artifact knows whether it is a component or a shared library, so `header`
takes the executor there. A module linked into the host has no header: it is
registered from its exports and described by its record.

## Build the artifact

The crate built as the artifact names the modules it carries, once, at its
root:

```rust
arora_module::artifact!(polly);
```

Built for `wasm32-wasip2`, the artifact is a component of the `arora:module`
world, which an engine loads under the `wasm-component` executor: `functions`
lists the functions of the modules it names, and `dispatch` routes a call to
the one that exports it. One artifact may carry several modules
(`artifact!(test_rust_wasm, compass)`), each loaded under its own header.
Built for another target, `artifact!` expands to nothing, and a shared library
built for the host exports each function as `arora_function_<id>`.

The exports go wherever the crate that writes `artifact!` is linked: a library
that another crate builds into its own artifact leaves `artifact!` to that
crate, or the two sets of exports collide at link time.

## A module in its own file

An attribute on `mod polly;` cannot see the file's items, so a module in its
own file ends with the aggregate naming its exports:

```rust
use arora_module::{declare_module, export};

declare_module! {
  id = "a1a6bb9a-…", name = "polly", version = "0.1.0",
  exports = [hello_world, say]
}
```

## Types

A parameter or return type is a primitive (`bool`, the integers, `f32`,
`f64`, `String`), an `arora_types::value::Value` (any value, as the dynamic
key-value type), a type deriving [`AroraType`](../arora-types/readme.md),
which gives it its `Value` conversions and the version a frozen signature pins
it at, or a `Vec<T>` of a primitive or an `AroraType`. Maps, tuples, `Box` and
nested arrays are refused: the record vocabulary has no form for them.

An `Option<T>` of any of those but an array is an **optional** parameter or
return. A caller may leave an optional argument out, or send
`Value::Option(None)`, and the function receives `None`; a present argument
arrives wrapped in `Value::Option` or as its bare element, the element's type
checked either way. Any other parameter is required: a call without it fails,
naming the parameter.

`scope = SCOPE` names the
[`Scope`](../arora-types/src/record/scope.rs) the module's records live in, and
the types it names that state no scope of their own live there too; without
it, they live at the root. [`docs/records.md`](../../docs/records.md) explains
scopes and records.

## Contracts: several modules, one set of functions

A **contract** declares functions that several modules implement, each under
its own module id: one `say`, served by a cloud speech provider on one device
and by a local one on another. It is a trait whose methods have no body and
take `&mut self`:

```rust
#[arora_module::contract(name = "say")]
pub trait Say {
  #[export(id = "e1b4bda7-…")]
  fn say(&mut self,
         #[param(id = "fb3787f2-…")] text: String,
         #[param(id = "5d0c7e91-…")] voice: Option<String>) -> Status;
}

struct Cloud;
impl Say for Cloud { fn say(&mut self, text: String, voice: Option<String>) -> Status { … } }

let module = HostModule::from_exports(CLOUD_ID, say::exports(Cloud));
```

Beside the trait, the module `say` (the trait's name in snake case) holds:

| | |
|---|---|
| `say::ids` | per function, its id and its parameters' |
| `say::NAME` | the contract's name: `name = "…"`, or the module's |
| `say::descriptions()` | each function's name and frozen signature, by id — how a device describes them, whatever implements them |
| `say::record(parent)`, `say::type_records(scope)` | the frozen record of an implementation, and the records of the types it names |
| `say::exports(implementation)` | every function, callable on `implementation`, for `HostModule::from_exports` |

`&mut self` is the implementation the host module owns: a unit struct when it
keeps no state, which takes no memory; its fields otherwise, each host module
holding its own value, which the engine's one-call-at-a-time dispatch lets it
change without a lock. The alternatives are under *A contract's functions take
`&mut self`* in the [design decisions](../../docs/design_decisions.md). rustc
checks that each implementation provides every function with its declared
signature. A contract has no artifact entry points: an artifact exports one
module's functions, declared with `#[module]`.

## Calling a module that is not a Rust declaration

`module_from_header!` reads a resolved header, relative to the crate's
manifest directory, at expansion, and produces the same `ids` and stubs from
it, plus `NAME` and `HEADER_YAML`:

```rust
arora_module::module_from_header!(
  "headers/polly.yaml",
  types = ["325a5767-e344-4532-860e-0749bcf2e428" => Status]
);
let status = say(&mut engine, "hello".to_string())?;
```

A record type left out of `types` is passed as a `Value`.

## Checked at compile time

Two functions of one module or contract cannot share an id or a name, nor two
parameters of one function; the build fails naming both. A parameter's name
spells its id constant, so it is a Rust identifier. A declaration naming an
`executor` is refused. Every error a declaration can report has a case under
[`tests/compile-fail`](tests/compile-fail).

## Reference

**`#[module(…)]` and `declare_module!` arguments.** `id` (required), `name`
(required by `declare_module!`; `#[module]` defaults it to the Rust module's
name), `version` (`major.minor.patch`), `author`, `license`, `description`,
`executable_mime`, `scope = PATH`; `declare_module!` also takes
`exports = [f, …]`.

**`#[export(…)]` and `#[param(…)]` arguments.** `id` (required), and `name` to
declare a name other than the Rust identifier's.

**What a declaration does not express.** A declared module's header has no
imports: its functions are called, they do not call other modules. A Rust
guest calling other modules is declared in a `module.yaml` and generated by
[`arora-module-rust`](../arora-module-authoring/rust/readme.md).

**Crates.** The macros live in
[`arora-module-macros`](../arora-module-macros/readme.md); depend on this crate,
which re-exports them with everything their expansion reaches.
