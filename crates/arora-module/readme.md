# arora-module

Write an Arora module in Rust: the crate that implements a module also carries
its interface.

```rust
#[arora_module::module(id = "a1a6bb9a-…", name = "polly", version = "0.1.0")]
pub mod polly {
  #[export(id = "e1b4bda7-…")]
  pub fn say(#[param(id = "fb3787f2-…")] text: String) -> Status { … }
}
```

Ids are pinned in the attributes, in hex; a parameter is a plain Rust type, or
`&mut T` when the function writes through it. From that one declaration come:

| | |
|---|---|
| `polly::ids` | the module id, and per function its id and parameter ids |
| `polly::header(executor)` | the header a `module.yaml` is written from at export |
| `polly::record(parent)` | the frozen module record a store serves |
| `polly::exports()` | every export callable, for `HostModule::of::<polly::Module>()` |
| `polly::client::say(&mut bridge, …)` | the typed stub a caller programs against |
| `arora_function_<id>` | the entry point an executor looks up in the built artifact |

**The executor is not the declaration's to name.** Only the step that builds
the artifact knows whether it is native or wasm, so `header` takes it there. A
module linked into the host has no header at all: it is registered from its
exports and described by its record.

## The two forms

`#[module]` goes on an **inline** Rust module and finds the `#[export]`
functions itself. For a module in its own file, an attribute macro cannot see
the items, so the file ends with the aggregate naming them:

```rust
use arora_module::{declare_module, export};

declare_module! {
  id = "a1a6bb9a-…", name = "polly", version = "0.1.0",
  exports = [hello_world, say]
}
```

## Several modules, one set of functions: contracts

A **contract** declares functions that several modules implement, each under
its own module id: one `say`, served by a cloud speech provider on one device
and by a local one on another. It is a trait whose methods have no body and
take `&mut self`, the implementation the host module owns and calls them on:

```rust
#[arora_module::contract(name = "say")]
pub trait Say {
  #[export(id = "e1b4bda7-…")]
  fn say(&mut self,
         #[param(id = "fb3787f2-…")] text: String,
         #[param(id = "5d0c7e91-…")] voice: Option<String>) -> Status;
}

impl Say for Cloud { fn say(&mut self, text: String, voice: Option<String>) -> Status { … } }

let module = HostModule::from_exports(CLOUD_ID, say::exports(Cloud::new()));
```

Beside the trait, the module `say` (the trait's name in snake case) holds:

| | |
|---|---|
| `say::ids` | per function, its id and parameter ids |
| `say::NAME` | the contract's name, `name = "…"` or the module's |
| `say::descriptions()` | each function's name and frozen signature, by id — how a device describes them, whatever implements them |
| `say::record(parent)` | the frozen module record of an implementation |
| `say::exports(implementation)` | every function callable on `implementation`, for `HostModule::from_exports` |

### Why `&mut self`

The receiver names the implementation. A trait is implemented for a type, so
an implementation has one whether or not its methods take `self`; the
receiver adds a value of that type, which the host module owns. An
implementation with no state is a unit struct. It is zero-sized: the value
takes no memory, and nothing is ever read through the reference.

```rust
struct Cloud;

impl Say for Cloud { fn say(&mut self, text: String, voice: Option<String>) -> Status { … } }

let module = HostModule::from_exports(CLOUD_ID, say::exports(Cloud));
```

An implementation with state keeps it in its fields, and each host module
holds its own value: two devices in one process do not share it. The
reference is `&mut` because the engine calls a host module's functions one
at a time, with exclusive access, so the implementation changes its fields
without a lock. The alternatives, and why they were not taken, are under
*A contract's functions take `&mut self`* in the
[design decisions](../../docs/design_decisions.md).

rustc checks that each implementation provides every function with its
declared signature. A contract has no artifact entry points: an artifact
exports one module's functions, declared with `#[module]`.

## Calling a module that is not a Rust declaration

`module_from_header!` reads a resolved header at expansion and produces the
same ids and stubs from it:

```rust
arora_module::module_from_header!(
  "headers/polly.yaml",
  types = ["325a5767-e344-4532-860e-0749bcf2e428" => Status]
);
let status = say(&mut engine, "hello".to_string())?;
```

## What a type must be

A parameter or return type is a primitive, a `Vec<T>` of one, an
`arora_types::value::Value` (anything, as the dynamic key-value type), or a
type deriving [`AroraType`](../arora-types/readme.md), which also gives it the
`Value` conversions and the version a frozen signature pins it at. Maps are
refused: the record vocabulary has no form for them.

An `Option<T>` of any of those but an array is an **optional** parameter or
return. A caller may leave an optional argument out, or send
`Value::Option(None)`, and the function receives `None`. A present argument
arrives wrapped in `Value::Option`, or as its bare element; either way the
element's type is checked. Any other parameter is required: a call without it
fails, naming the parameter.

## Checked at compile time

Two functions of one module or contract cannot share an id or a name, nor two
parameters of one function; the build fails naming both. A parameter's name
spells its id constant, so it is a Rust identifier.
