# arora-types

The vocabulary every part of Arora shares: the values that cross a module
boundary, the types that describe them, the calls that carry them, and the
records that name types and modules from one machine to another. It depends on
no engine, so a tool, a binding or a remote client can depend on it alone.

## Values

A [`Value`](src/value.rs) is any piece of data Arora moves: a function's
argument or result, the state of a key in a data store, a parameter's default.
It is one enum:

| Variants | Hold |
|---|---|
| `Unit`, `Boolean`, `U8` … `U64`, `I8` … `I64`, `F32`, `F64`, `String`, `Uuid` | one primitive |
| `Option` | a value, or none |
| `ArrayBoolean`, `ArrayU8` … `ArrayF64`, `ArrayString` | a homogeneous array of a primitive |
| `ArrayValue` | an array of values of any shape |
| `Structure` | the fields of a structure, each under its **field id**, under the structure's **type id** |
| `Enumeration` | one variant of an enumeration, under its **variant id**, with that variant's payload |
| `ArrayStructure`, `ArrayEnumeration` | an array of structures or enumerations of one type, its id stated once |
| `KeyValue` | fields under their **names**, for data no declared type describes |

Data is identified by UUID, not by name. A `Structure` states which type it is
by that type's id and holds each field under the field's id, so renaming a type
or a field in source changes nothing a reader sees. Each primitive and built-in
form has a fixed, well-known id ([`ty::F64_ID`](src/ty/mod.rs),
`ty::STRING_ID`, …, `ty::KEY_VALUE_ID`, `ty::UUID_ID`). `Value::type_uuid`
answers a value's id: the well-known one, or its type's for a structure or an
enumeration.

Rust primitives, `String`, `Uuid`, an `Option` of any of them and a `Vec` of a
primitive convert into a `Value` with `From`:

```rust
use arora_types::value::Value;

assert_eq!(Value::from(1.5f64), Value::F64(1.5));
assert_eq!(Value::from(vec![1u8, 2]), Value::ArrayU8(vec![1, 2]));
assert_eq!(Value::from(None::<f64>), Value::Option(None));
```

`Value::kind` answers which variant a value is, as a
[`value::Type`](src/value.rs); `Value::conforms_to` compares it exactly, with no
conversion — an `F32` is not of `F64`, an `ArrayValue` of floats not of
`ArrayF64`.

A `Value` serializes with serde, tagged by its variant: `{"f64": 1.5}`,
`{"struct": {"id": "…", "fields": [{"id": "…", "value": {"i32": 3}}]}}`.

## Types

The id a structure or an enumeration carries names a **type definition**, a
[`ty::low::Type`](src/ty/low.rs): its id, name, description and kind.

- `TypeKind::Structure` — its fields, each a name and a type reference under
  the field's id, in declared order.
- `TypeKind::Enumeration` — its variants, each a name and a payload type
  reference under the variant's id, in declared order.
- `TypeKind::Primitive` — a type reference to a primitive.

A **type reference**, [`module::low::TypeRef`](src/module/low.rs), names the
type of a field, a variant's payload, a parameter or a return value by id:

| `TypeRef` | Means |
|---|---|
| `Scalar { id }` | one value of type `id` |
| `Array { id }` | any number of values of type `id` |
| `FixedArray { id, len }` | exactly `len` values of type `id` |
| `Option { id }` | a value of type `id`, or none |
| `Map { key_id, value_id }` | a map, carried as a `KeyValue` |

A [`TypeRegistry`](src/ty/mod.rs) holds definitions by id, so a reference to a
structure or an enumeration can be followed; a primitive is known by its id
and needs no entry. With a definition in hand:

- `Type::validate` checks a value against it — the type id, every field once
  and of its declared type, a variant the enumeration declares;
- `Type::default_value` builds the value the type rests at. A field naming
  another structure or enumeration gets a shell of that type with no fields,
  so the default of a structure holding one does not validate;
- `value_serde::write_value` and `read_value` encode and decode a value in a
  wire format ([below](#wire-formats)).

## Rust types as Arora types

The [`AroraType`](src/arora_type.rs) trait is a Rust type describing itself:
`arora_type_id()`, `arora_type()` (its `ty::low::Type`), `register_types()` (it
and every type it names, into a `TypeRegistry`), `arora_type_version()`, and
its [record](#records). `#[derive(AroraType)]`
([`arora-types-derive`](../arora-types-derive/readme.md), re-exported here) writes
the implementation from the Rust definition, every id pinned in an attribute,
and adds `From<T> for Value` and `TryFrom<Value> for T`:

```rust
use arora_types::{value::Value, AroraType};

#[derive(Debug, PartialEq, AroraType)]
#[arora(id = "0a0a0a0a-0000-4000-8000-000000000001")]
struct Point {
    #[arora(id = "0a0a0a0a-0000-4000-8000-000000000002")]
    x: f64,
    #[arora(id = "0a0a0a0a-0000-4000-8000-000000000003")]
    y: f64,
}

let value = Value::from(Point { x: 1.0, y: 2.0 }); // a Structure under Point's ids
assert_eq!(value.type_uuid(), Point::arora_type_id());
assert!(Point::arora_type().validate(&value).is_ok());
assert_eq!(Point::try_from(value).unwrap(), Point { x: 1.0, y: 2.0 });
```

## Converting any serde type

[`value_serde`](src/value_serde/mod.rs) converts any `Serialize` /
`Deserialize` type to and from a `Value`, without a derive. Whether real ids are
available decides the form:

- **`to_value` / `from_value`** — no type is given, so a struct becomes a
  `KeyValue` carrying its field names, not a `Structure`. An id made by hashing
  a name would change the moment the type or the field is renamed; the names
  are the honest key. A map is a `KeyValue` too, a sequence an `ArrayValue`; an
  enum's unit variant is its name as a string, any other variant a one-entry
  `KeyValue` keyed by the variant's name.
- **`to_value_seeded` / `from_value_seeded`** — a `ty::low::Type` and its
  `TypeRegistry` supply the ids, so a struct becomes a `Structure` under the
  declared type and field ids, and a sequence declared as an array takes its
  typed form (`ArrayF64`, `ArrayStructure`, …).

No ids, a `KeyValue` keyed by names; declared ids, a `Structure` keyed by ids.

```rust
use arora_types::{value::Value, value_serde, AroraType};

#[derive(serde::Serialize, serde::Deserialize, AroraType)]
#[arora(id = "0a0a0a0a-0000-4000-8000-000000000001")]
struct Point {
    #[arora(id = "0a0a0a0a-0000-4000-8000-000000000002")]
    x: f64,
    #[arora(id = "0a0a0a0a-0000-4000-8000-000000000003")]
    y: f64,
}

let named = value_serde::to_value(&Point { x: 1.0, y: 2.0 }).unwrap();
assert!(matches!(named, Value::KeyValue(_)));

let (ty, registry) = Point::arora_type_with_registry();
let typed = value_serde::to_value_seeded(&Point { x: 1.0, y: 2.0 }, &ty, &registry).unwrap();
assert!(matches!(typed, Value::Structure(_)));
```

## Calls

A [`Call`](src/call.rs) invokes one function: `module_id`, the module exporting
it; `id`, the function's id; and `args`, each argument a `StructureField` under
its parameter's id. Its [`CallResult`](src/call.rs) carries `ret`, the return
value, and `mutated`, the values of the call's mutable parameters after the
call. A parameter is an input, and a `mutable` one an output too: a call has in
and out parameters, and hands its outputs back to its caller.

[`CallBridge`](src/call.rs) is how code calls functions without depending on
what answers them:

- `arora_call` dispatches a `Call` to the module it names; a call naming no
  module is refused. `arora_call_borrowed` does the same with borrowed
  arguments.
- `arora_register_callable` registers a [`Callable`](src/call.rs) and returns
  its `CallableId`; `arora_call_indirect` invokes it by that handle, and
  `arora_unregister_callable` drops it.

The engine implements `CallBridge`; the behavior tree and module-shaped
libraries consume it, which is why they run against any implementation.
[`docs/dispatch.md`](../../docs/dispatch.md) covers direct and indirect
dispatch. A failed call is a `CallError`.

## Modules

A module's interface is the functions it exports — each an id, a name,
parameters (an id, a name, a type reference, `mutable`, an optional
`default_value`) and a return type reference — and the functions of other
modules it imports. Four forms carry it:

| Form | What it is |
|---|---|
| [`module::high::ModuleDefinition`](src/module/high.rs) | The interface as an author writes it in a `module.yaml`: each type named by a string — a primitive's name (`u32`, `str`), a record's path (`arora.Status`) or an id. |
| [`module::low::Header`](src/module/low.rs) | The same interface resolved, every type reference an id. Module authoring writes it as `header.json` beside the module's artifact, and as a `module.yaml` `arora-cli --header` reads. |
| [`module::low::ModuleDefinition`](src/module/low.rs) | A `Header` with the module's executable bytes: what an engine loads. |
| [`module::declared::AroraModule`](src/module/declared.rs) | The trait a module declared in Rust implements, through the `arora-module` macros: `header(executor)`, `record(parent)`, `scope()`, `type_records()`, and `exports()`, each an [`AroraFunction`](src/module/declared.rs) ready to call. |

## Records

Types and modules travel between machines as **records**: versioned
declarations of a structure, an enumeration or a module, each under a stable id,
organized in folders whose top level is the publisher's scope.
[`record`](src/record/mod.rs) holds them in two forms — *unfrozen*, references
carrying version requirements, and *frozen*, every reference pinned to one
version, the form record files and the store carry — and `Freeze` with a
`Resolver` turns one into the other. `AroraType::record` and
`AroraType::register_records`, and `AroraModule::record` and
`AroraModule::type_records`, produce the records of a type or module declared
in Rust. [`docs/records.md`](../../docs/records.md) explains the model, and
[`arora-registry`](../arora-registry/readme.md) holds records.

## Data

[`data`](src/data/mod.rs) is the vocabulary of a device's shared state, which
its HAL, bridges and behaviors read and write:

- [`Key`](src/data/state.rs) — the name of a value: a `/`-separated path
  (`arora/time`, `display/face/compressed`). A trailing `.attribute` on the
  last segment (`head_yaw.target_position`) is a naming convention, nothing
  more: stores, bridges and access checks compare the whole path.
- [`State`](src/data/state.rs) — keys and their values;
  [`StateChange`](src/data/state.rs) — keys set and keys unset.
- [`DataStore`](src/data/store.rs) — a shared, observable store: `read`,
  `write`, `snapshot`, `subscribe` and `subscribe_prefix` (a `Subscription`
  whose first change is the current state), and `slot`, a
  [`Slot`](src/data/store.rs) — a handle on one key's cell, resolved once.
  [`arora-simple-data-store`](../arora-simple-data-store) implements it.
- [`KeyMeta`](src/data/store.rs) — what a key is beyond its value: the
  `value::Type` it holds, its range, unit, resting value and description, and
  whether a writer outside the device may set it. A key is closed to remote
  writers unless its meta opens it (`KeyMeta::editable`, per key with
  `DataStore::set_meta` or per subtree with `DataStore::set_prefix_meta`); the
  device's own writers are never checked.

## Wire formats

A wire format is a [`ValueWriter`](src/value_serde/walk.rs) / `ValueReader`
pair: the primitive reads and writes, bulk arrays, and the framing of
structures, arrays of structures and options. `write_value` and `read_value`
drive it over a `ty::low::Type` and a `TypeRegistry` — fields in declared
order, the value validated against the type as it goes — so a format never
matches on `Value` variants or recurses a type itself. Its one choice is how
much it frames:

- **self-describing** — arora-buffers'
  [`froto_checked_value`](../arora-buffers/src/froto_checked_value.rs) writes a
  type tag before each datum and, on read, checks it against the type the walk
  asks for;
- **positional** — the ROS 2 CDR codec of
  [`arora-msgs-ros2`](../arora-msgs-ros2/src/cdr.rs) writes no type
  information: the type alone fixes the layout, so a structure and its fields
  write nothing, a variable-length array or an optional writes only its CDR
  sequence count (an optional's is 0 or 1), and a fixed-length array only its
  elements.

The walk covers primitives, strings, structures, variable- and fixed-length
arrays and options. A value whose type is an enumeration, a map or a uuid is
refused with an error.

## JavaScript bindings

Built for `wasm32-unknown-unknown`, the crate exports two wasm-bindgen classes
from [`wasm_value`](src/wasm_value.rs): `Value`, wrapping a `Value`, and
`ValueType`, its variant. arora-types builds as an `rlib` only, so they reach
JavaScript through a `cdylib` that links it, such as
[`arora-web`](../arora-web/readme.md).

```typescript
const speed = new Value(ValueType.F64, 3.14); // integers are range-checked
speed.type;                    // ValueType.F64
speed.get();                   // 3.14
speed.set("fast");             // throws: not an f64
Value.from(42);                // F64: a number defaults to f64
Value.from([1, 2, 3]);         // ArrayF64
Value.from([42, "text"]);      // ArrayValue
Value.from({ name: "Alice" }); // KeyValue
Value.from(null);              // Option, none
```

| `Value` | In JavaScript |
|---|---|
| `Unit` | `undefined` |
| `Option` | its value, or `null` |
| integers, floats | `number`; a 64-bit integer loses precision past 2^53 |
| `Uuid` | its string |
| arrays | `Array` |
| `KeyValue` | a plain object |
| `Structure` | `{ id, fields: { <field id>: value } }` |
| `Enumeration` | `{ id, variant_id, value }` |

`Value.from` reads an object with a uuid `id` and a `variant_id` as an
enumeration, one with a uuid `id` and `fields` as a structure, and any other
object as a `KeyValue`. `ValueType`'s numbers are its own, not the tags of the
[arora-buffers](../arora-buffers/readme.md) wire format.

## Reference

**Features.** `derive` (default) re-exports `#[derive(AroraType)]`; without it
the crate has no proc-macro dependency, and `AroraType` is implemented by hand.

**Three meanings of "type".**

| Item | What it describes |
|---|---|
| [`value::Type`](src/value.rs) | which variant a value is — what `KeyMeta` states and `Value::conforms_to` checks |
| [`ty::low::Type`](src/ty/low.rs) | a type definition, its references by id — what validation, defaults and the wire walk read. [`ty::high::Type`](src/ty/high.rs) is the same definition naming types by string. |
| [`record::ty::UnfrozenTy` / `FrozenTy`](src/record/ty.rs) | the type of a field or parameter inside a record: a primitive, or a reference to another record with its version requirement or its pinned version |

**Ids.** `Uuid` is re-exported. `gen_uuid_from_str` parses a UUID, or hashes
any other string into one; `gen_bb_uuid` makes a random one. `SemanticVersion`
is the `major.minor.patch` a module definition and a header carry.

## License

Apache-2.0.
