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
`Value` conversions and the version a frozen signature pins it at. `Option` and
maps are refused: the record vocabulary has no form for them.
