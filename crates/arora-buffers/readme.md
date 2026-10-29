# arora-buffers

The binary form of an Arora [`Value`](../arora-types/readme.md#values) at a
module boundary: an **Arora Buffer**. A host hands a guest module a call's
arguments as one buffer and gets the result back as another — as the component
model's `list<u8>`, or through a native module's C ABI. A buffer is
self-describing: it carries its own type tags and ids, so a reader rebuilds the
value with no schema in hand.

Hosts and guests built separately, in Rust and in C++, all read and write this
layout: it is a contract, and this page describes it.

## Smallest use

```rust
use arora_buffers::{froto_value, well_formed};
use arora_types::value::Value;

let bytes = froto_value::serialize(&Value::F64(1.5));
assert!(well_formed(&bytes).is_ok());
assert_eq!(froto_value::deserialize(&bytes), Value::F64(1.5));
```

`froto_value::deserialize` panics on bytes that are not a buffer, and a
guest's panic is a trap. Code handed a buffer it did not write — a guest, given
its caller's argument — checks it with `well_formed` first, which says why a
buffer is not one without decoding it. `value_end` steps over one value the
same way, to read a buffer in place.

## A call as buffers

A guest's `dispatch(method, arg)` receives a call's arguments as one structure
under the function's id, each argument a field under its parameter's id. It
returns one structure under the same id: the return value as a field under the
function's id, then each mutated parameter under its own id. Those are the
`args`, `ret` and `mutated` of an
[`arora_types::call`](../arora-types/readme.md#calls) `Call` and `CallResult`.

## Buffer layout

A buffer is a length-prefixed byte stream:

```text
┌──────────────┬──────────────────────────────┐
│ size: u32 LE │ payload (one tagged value)    │
└──────────────┴──────────────────────────────┘
```

- **Size prefix** — the first 4 bytes are the total buffer length (prefix
  included) as a little-endian `u32`. Capped at 32 bits to match WASM's memory
  model.
- **Payload** — exactly one *value*; composite values (structure, array, map)
  nest further values.
- **Endianness** — every multi-byte scalar is little-endian.
- **Alignment** — `ALIGNMENT = 8`. The format is otherwise **packed** (no
  per-field padding). The *only* aligned region is the bulk body of a primitive
  array (see below), padded to an 8-byte boundary so it can be read as a slice.
  Padding is measured from the buffer **start** — the size prefix counts.

## Values

Every value is a **1-byte type tag** followed by its payload:

| tag | value | payload after the tag |
|----|----|----|
| 0 `UNIT` | unit | — |
| 1 `BOOLEAN` | bool | 1 byte (`0`/`1`) |
| 2–5 `U8`/`U16`/`U32`/`U64` | unsigned int | 1/2/4/8 bytes LE |
| 6–9 `I8`/`I16`/`I32`/`I64` | signed int | 1/2/4/8 bytes LE |
| 10–11 `F32`/`F64` | float | 4/8 bytes LE |
| 12 `STRING` | string | `u32` byte length + UTF-8 bytes (no NUL) |
| 18 `UUID` | uuid | 16 bytes |
| 13 `STRUCTURE` | structure | 16-byte type id + `u32` field count + fields |
| 14 `ENUMERATION` | enumeration | 16-byte type id + 16-byte variant id + value |
| 17 `OPTION` | option | 1 presence byte (`0`/`1`); the value follows if `1` |
| 16 `MAP` | keyvalue | 16-byte id + `u32` count + entries |
| 15 `ARRAY` | array | element tag + `u32` count + elements |
| 19 `VALUE` | dynamic | the element tag of a heterogeneous array |
| 20 `ERROR` | error | `u32` length + UTF-8 message |

The tags are the `TYPE_*` constants of [`format`](src/format.rs).

### Structure

```text
[STRUCTURE][type id: 16][field_count: u32]   then per field:   [field id: 16][value]
```

Fields are in the type's declared order; each field value is a full tagged value.

### Enumeration

```text
[ENUMERATION][type id: 16][variant id: 16][value]
```

### Map (keyvalue)

```text
[MAP][id: 16][field_count: u32]   then per entry:   [key_len: u32][key bytes][field id: 16][value]
```

Entries are written sorted by key, for a deterministic encoding.

### Array — the element type is tagged once

```text
[ARRAY][element tag: u8][count: u32]   then the elements
```

The element bodies depend on the element tag:

- **Primitive** (`BOOLEAN`/`U*`/`I*`/`F*`) — one 8-byte alignment pad, then the
  elements packed raw little-endian, back to back. This is the one aligned
  region, so the block can be read as a slice.
- **String** (`STRING`) — the elements back to back, each `u32 len + bytes`. No
  per-element tag, no alignment.
- **Structure** (`STRUCTURE`) — a 16-byte element type id, then per element a
  *headerless struct body*: `u32 field_count` + `[field id: 16][value]`×. The
  struct type is named once (in the header), never re-tagged per element.
- **Enumeration** (`ENUMERATION`) — a 16-byte element type id, then per element
  `[variant id: 16][value]`.
- **Dynamic** (`VALUE`) — a heterogeneous sequence: each element is a full
  self-describing tagged value. Used for a generic `Vec<T>` whose element type
  is not fixed up front.

## Which serializer?

Four modules map between this layout and different in-memory shapes. For the
same data they all produce **byte-identical** output.

| module | maps | ownership | ids | reach for it when |
|---|---|---|---|---|
| [`froto_value`](src/froto_value.rs) | `arora_types::Value` ⇄ bytes | owned | `Uuid` | you hold a runtime `Value` and want it on the wire — the canonical path |
| [`froto_borrowed_value`](src/froto_borrowed_value.rs) | a borrowing `Value<'a>` ⇄ bytes | **borrowed / zero-copy** | raw `[u8]` | you want zero-copy reads (`Cow<[f64]>` straight from the buffer), or to (de)serialize a value from YAML/JSON (it derives `serde`); it has no option, map, uuid, error or mixed-type array |
| [`froto_serde`](src/froto_serde.rs) | any `serde` type `T` ⇄ bytes (`to_bytes` / `from_bytes`) | owned | **names, not ids** | you have a concrete Rust type and want it on the wire without building a `Value` |
| [`froto_checked_value`](src/froto_checked_value.rs) | `arora_types::Value` ⇄ bytes, **type-directed** | owned | `Uuid` (declared) | you want the encoding *validated* against a runtime `ty::low::Type`, through the `arora_types::value_serde` walk other wire formats (ROS 2 CDR) share |

(`froto` = *from/to*: each converts one in-memory shape to and from the bytes.)
`froto_checked_value` adds a type contract on top of the bytes `froto_value`
writes; a test pins the two equal. Its `BuffersValueWriter` and
`BuffersValueReader` are what `value_serde::write_value` and `read_value` drive.
`serde_uuid` is `froto_value` under another name, the one generated module code
and the engine reach it by.

**Untyped vs. typed — keyvalue vs. structure.** `froto_serde` has no declared
type, so a struct is written as a **keyvalue** (the map form above, keyed by
field name), exactly as `arora_types::value_serde::to_value` makes a
`Value::KeyValue` of it. An id hashed from a name would break the moment a
field or struct is renamed. The **structure** form — real type and field ids —
is what the *typed* path writes: `froto_checked_value`, driven by a declared
`ty::low::Type` whose ids come from `#[derive(AroraType)]`. Same rule as the
value layer: **no ids ⇒ keyvalue; declared ids ⇒ structure.**

## C ABI

The reader and writer are a `#[no_mangle] extern "C"` surface
([`ffi`](src/ffi/mod.rs)) over the safe Rust
[`BufferWriter`](src/writer.rs) / [`BufferReader`](src/reader.rs): the
`arora_buffer_writer_*` and `arora_buffer_reader_*` functions, and
`arora_buffer_well_formed`. The crate builds as a `staticlib` as well as an
`rlib`; its build script writes the C header `target/include/arora/buffers.h`,
naming the tags `ARORA_BUFFER_TYPE_*`. A C++ guest links the `wasm32-wasip2`
static library, with the C++ helpers of [`libs/cpp`](../../libs/cpp) over it.

## Modules

- [`format`](src/format.rs) — the type tags, alignment and size-prefix
  constants: the layout above, in code.
- [`writer`](src/writer.rs) / [`reader`](src/reader.rs) — the low-level
  `BufferWriter` / `BufferReader` cursor.
- [`well_formed`](src/well_formed.rs) — `well_formed` and `value_end`, which
  walk a buffer without decoding it, values nested at most `MAX_DEPTH` deep.
- [`ffi`](src/ffi/mod.rs) — the C ABI.
- [`froto_value`](src/froto_value.rs) / [`froto_borrowed_value`](src/froto_borrowed_value.rs) /
  [`froto_serde`](src/froto_serde.rs) / [`froto_checked_value`](src/froto_checked_value.rs) —
  the four serializers above.
