# Arora Types

Shared type definitions for the [Arora](https://github.com/semio-ai/arora-engine)
framework: the vocabulary the engine, modules, registries and clients use to
describe modules, types and runtime values. It carries no engine dependencies, so
it is safe to depend on from tools, bindings and remote clients.

## High-level vs. low-level types

High-level types use names to reference other entities. Names are meant to be
resolved using a
[registry](https://github.com/semio-ai/arora-engine/blob/main/crates/arora-registry/readme.md),
or local indexes associating [UUIDs](https://docs.rs/uuid) to names.

Low-level types are produced for contexts where [UUIDs](https://docs.rs/uuid) are
sufficient, if not more efficient for looking them up.

## Module

The "high-level"
[`ModuleDefinition`](https://github.com/semio-ai/arora-engine/blob/main/crates/arora-types/src/module/high.rs)
completely describes a module to implement. It is usually saved as a `module.yaml`
file (using [`serde_yaml`](https://docs.rs/serde_yaml)). It can be used by the
code generators of
[`arora-module-cli`](https://github.com/semio-ai/arora-engine/blob/main/crates/arora-module-authoring/cli/readme.md)
to produce the proper bindings for a module.

The "low-level" format of a module is called a
[`Header`](https://github.com/semio-ai/arora-engine/blob/main/crates/arora-types/src/module/low.rs),
and is produced by the code generators under the file name `header.yaml`. It is
used to load the module in the engine, with
[`arora-cli`](https://github.com/semio-ai/arora-engine/blob/main/crates/arora-cli/readme.md).

Modules may export symbols, so that they can be called by any client. Modules may
also declare symbols to import from other modules, so that the right bindings are
made available in the implementation. The only symbols supported so far are
functions. Their declaration may involve references to existing [types](#type-ty):

- directly (`TypeRef::Scalar`)
- as the element type of an array (`TypeRef::Array`)
- as the key or value type of a map (`TypeRef::Map`). This kind of reference is
  not used in this project, in practice.

## Type (`ty`)

Structured types can be described in both
[high-level](https://github.com/semio-ai/arora-engine/blob/main/crates/arora-types/src/ty/high.rs)
or
[low-level](https://github.com/semio-ai/arora-engine/blob/main/crates/arora-types/src/ty/low.rs)
ways, so that they can be used in both high-level or low-level modules. This
library can describe:

- [primitive types](https://github.com/semio-ai/arora-engine/blob/main/crates/arora-types/src/ty/mod.rs),
  equivalent in Rust to: `bool`, `u8`, `u16`, `u32`, `u64`, `i8`, `i16`, `i32`,
  `i64`, `f32`, `f64`, `String`.
- enumerations, similar to Rust `enum`s: each variant can hold a value of any
  other type, and does not necessarily translate into an integer.
- structures, similar to Rust `struct`s and C / C++ PODs: each field has a name
  and holds a value.

## Value

A [`Value`](https://github.com/semio-ai/arora-engine/blob/main/crates/arora-types/src/value.rs)
describes a value defined in the low-level [types](#type-ty). It is generic and
can also be serialized (using [`serde`](https://docs.rs/serde)). For other kinds
of conversions a common error type is suggested:
`arora_types::value::ConversionError`.

`Value`s are useful at runtime to pass arguments to functions, but also to
describe `default_value`s for function parameters.

> Note: we call "parameter" the declaration of what a function may accept as
> inputs (or outputs, if `mutable`). We call "argument" the actual value passed to
> the function.

## Web Bindings

The `Value` type is exposed to JavaScript/TypeScript via WebAssembly bindings
(published as the `@semio-ai/arora-types` npm package). This lets you work with
Arora values from JavaScript environments.

### Using from JavaScript/TypeScript

```typescript
import { Value, ValueType } from "@semio-ai/arora-types";

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

### Type mapping

| Rust Type                 | WASM `ValueType`          | JavaScript Type | Notes                            |
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
| `Uuid`                    | `Uuid`                    | `string`        | UUID string                      |

### Building and testing the bindings

```bash
# Build the WASM module (creates a pkg/ directory for wasm32-unknown-unknown)
npm run build:wasm
# Run the JS integration tests against the locally built pkg/
npm test
```

The integration tests verify that all `ValueType` values are exposed, that value
construction/retrieval works for every primitive type, that integer range and
`set()` type checks hold, and that arrays, auto-detection and `KeyValue` objects
behave as expected.

## License

MIT.
