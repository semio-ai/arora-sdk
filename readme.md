# Arora Schemas

Common schemas used by the engine.
These schemas are somewhat redundant with
[Semio Records](https://github.com/semio-ai/semio-record),
but it is quite a lot of work to switch to Semio Records.

## High-level vs. low-level schemas

High-level schemas use names to reference other entities.
Names are meant to be resolved using a
[registry](../arora-registry/readme.md),
or local indexes associating
[UUIDs](https://docs.rs/uuid/latest/uuid/index.html) to names.

Low-level schemas are produced for contexts where
[UUIDs](https://docs.rs/uuid/latest/uuid/index.html) are sufficient,
if not more efficient for looking them up.

## Module

The "high-level" [`ModuleDefinition`](src/module/high.rs)
is used to describe completely a module to implement.
It is usually saved as a `module.yaml` file
(using [`serde-yaml`](https://docs.serde.rs/serde_yaml/index.html)).
It can be used by the code generators of
[`arora-module-cli`](../arora-module-cli/readme.md)
to produce the proper bindings for a module.

The "low-level" format of a module is called a
[`Header`](src/module/low.rs),
and is produced by the code generators under the file name `header.yaml`.
It is used to load the module in the engine,
with [`arora-cli`](../arora-cli/readme.md).

Modules may export symbols,
so that they can be called by any client.
Modules may also declare symbols to import from other modules,
so that the right bindings are made available in the implementation.
The only symbols supported so far are functions.
Their declaration may involve references to existing [types](#type):
- directly (`TypeRef::Scalar`)
- as the element type of an array (`TypeRef::Array`)
- as the key or value type of a map (`TypeRef::Map`).
  This kind of reference is not supported in this project, in practice.

## Type (`ty`)

Structured types can be described in both
[high-level](src/ty/high.rs) or [low-level](src/ty/low.rs) ways,
so that they can be used in both high-level or low-level modules.
This library can describe:
- [primitive types](src/ty/mod.rs), equivalent in Rust to:
  `bool`, `u8`, `u16`, `u32`, `u64`, `i8`, `i16`, `i32`, `i64`,
  `f32`, `f64`, `String`.
- enumerations, similar to Rust `enum`s:
  each variant can hold a value of any other type,
  and do not necessarily translate into an integer.
- structures, similar to Rust `struct`s and C / C++ PODs:
  each field has a name and holds a value.

## Value

A [`Value`](src/value.rs) describes a value defined
in the low-level [type](#type) schema.
It is generic and can also be serialized
(using [`serde`](https://docs.serde.rs/serde/).
For other kind of conversions
a common error type is suggested:
`arora-schema::value::ConversionError`.

[`Value`s](src/value.rs) are useful at runtime
to pass arguments to functions,
but also to describe `default_value`s
for function parameters.

> Note: we call "parameter" the declaration of
> what function may accept as inputs (or outputs, if `mutable`).
> We call "argument" the actual value passed to the function.
