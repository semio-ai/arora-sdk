# Arora Types

Common types used by the engine.
These types are somewhat redundant with
[Semio Records](https://github.com/semio-ai/semio-record),
but it is quite a lot of work to switch to Semio Records.

## High-level vs. low-level types

High-level types use names to reference other entities.
Names are meant to be resolved using a
[registry](../arora-registry/readme.md),
or local indexes associating
[UUIDs](https://docs.rs/uuid/latest/uuid/index.html) to names.

Low-level types are produced for contexts where
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
Their declaration may involve references to existing [types](#type-ty):

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
in the low-level [types](#type-ty).
It is generic and can also be serialized
(using [`serde`](https://docs.serde.rs/serde/).
For other kind of conversions
a common error type is suggested:
`arora_types::value::ConversionError`.

[`Value`s](src/value.rs) are useful at runtime
to pass arguments to functions,
but also to describe `default_value`s
for function parameters.

> Note: we call "parameter" the declaration of
> what function may accept as inputs (or outputs, if `mutable`).
> We call "argument" the actual value passed to the function.

## Web Bindings

The `Value` type is exposed to JavaScript/TypeScript via WebAssembly bindings. This allows you to work with Arora values from JavaScript environments.

### Running Integration Tests

```bash
# Build the WASM module
npm run build:wasm # Creates a pkg/ directory
npm test # Tests the pkg/ built locally
```

This will:

1. Compile the Rust code to WebAssembly (`wasm32-unknown-unknown` target)
2. Generate JavaScript bindings and a ready-to-publish NPM package in the `pkg/` directory

The integration tests verify that:

- All `ValueType` enum values are properly exposed
- Value construction and retrieval works for all primitive types
- Integer range validation works correctly
- Array types (homogeneous and mixed) work as expected
- Auto-detection from JavaScript values works correctly
- Type checking in `set()` method works
- KeyValue objects can be created from plain JavaScript objects

### Publishing the NPM Package

The package is published as `@semio-ai/arora-types`.

**CI publishes automatically:**

- **Pull requests** publish a pre-release to the GitHub Packages registry.
- **Pushes to `main`** publish the release version to the public NPM registry.

**To publish manually** (requires `wasm-pack` and NPM credentials):

```bash
# Build and publish to npm in one step
wasm-pack publish --scope semio-ai --target bundler
```

> **Prerequisites:**
>
> - `wasm-pack` must be installed (`cargo install wasm-pack`)
> - The `wasm32-unknown-unknown` target must be available (`rustup target add wasm32-unknown-unknown`)
> - You must be logged into npm (`npm login`) or have `NPM_TOKEN` set

### Using from JavaScript/TypeScript

```typescript
import { Value, ValueType } from './pkg/arora_types.js';

// Create values with explicit types
const num = new Value(ValueType.F64, 3.14);
const str = new Value(ValueType.String, "hello");
const bool = new Value(ValueType.Boolean, true);

// Get the type and value
console.log(num.type);  // ValueType.F64
console.log(num.get()); // 3.14

// Auto-detect types from JavaScript values
const autoNum = Value.from(42);        // Detects as F64
const autoStr = Value.from("world");   // Detects as String
const autoBool = Value.from(false);    // Detects as Boolean
const autoNull = Value.from(null);     // Converts to Unit

// Arrays
const numArr = new Value(ValueType.ArrayF64, [1.0, 2.0, 3.0]);
const mixedArr = Value.from([42, "text", true]); // ArrayValue

// Key-value objects
const obj = Value.from({ name: "Alice", age: 30 });
console.log(obj.type); // ValueType.KeyValue
console.log(obj.get()); // { name: "Alice", age: 30 }

// Mutable values with type checking
const val = new Value(ValueType.I32, 10);
val.set(20);  // OK
val.set("x"); // Error: type mismatch
```


### Type Mapping

| Rust Type                 | WASM ValueType            | JavaScript Type | Notes                            |
|---------------------------|---------------------------|-----------------|----------------------------------|
| `()`                      | `Unit`                    | `null`          |                                  |
| `bool`                    | `Boolean`                 | `boolean`       |                                  |
| `u8`, `u16`, `u32`, `u64` | `U8`, `U16`, `U32`, `U64` | `number`        | Range validated                  |
| `i8`, `i16`, `i32`, `i64` | `I8`, `I16`, `I32`, `I64` | `number`        | Range validated                  |
| `f32`, `f64`              | `F32`, `F64`              | `number`        | Default for auto-detection       |
| `String`                  | `String`                  | `string`        |                                  |
| `Option<T>`               | `Option`                  | `T \| null`     |                                  |
| `Vec<T>`                  | `Array*`                  | `T[]`           | Typed arrays                     |
| `Value[]`                 | `ArrayValue`              | `any[]`         | Mixed-type arrays                |
| `KeyValue`                | `KeyValue`                | `object`        | Plain objects                    |
| `Structure`               | `Structure`               | `object`        | With `id` and `fields`           |
| `Enumeration`             | `Enumeration`             | `object`        | With `id`, `variant_id`, `value` |
| `Uuid`                    | `Uuid`                    | `string`        | UUID string format               |
