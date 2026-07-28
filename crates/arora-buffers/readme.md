# Arora Buffers

`arora-buffers` is the binary exchange format used across arora **module
boundaries** — host ↔ WASM guest, and any producer/consumer that speaks the ABI.
It encodes an arora `Value` (or, via serde, a Rust type) into a **self-describing**
`[u8]` and reads it back. Self-describing means the bytes carry their own type
tags and ids, so a reader can reconstruct the value with no schema in hand.

## Buffer layout

A buffer is a length-prefixed byte stream:

```
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

### Structure

```
[STRUCTURE][type id: 16][field_count: u32]   then per field:   [field id: 16][value]
```

Fields are in the type's declared order; each field value is a full tagged value.

### Enumeration

```
[ENUMERATION][type id: 16][variant id: 16][value]
```

### Map (keyvalue)

```
[MAP][id: 16][field_count: u32]   then per entry:   [key_len: u32][key bytes][field id: 16][value]
```

Entries are written sorted by key, for a deterministic encoding.

### Array — the element type is tagged **once**

```
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

Four modules map between this wire format and different in-memory shapes. For the
same data they all produce **byte-identical** output.

| module | maps | ownership | ids | reach for it when |
|---|---|---|---|---|
| [`froto_value`](src/froto_value.rs) | `arora_types::Value` ⇄ bytes | owned | `Uuid` | you hold a runtime `Value` and want it on the wire — the canonical path |
| [`froto_borrowed_value`](src/froto_borrowed_value.rs) | a borrowing `Value<'a>` ⇄ bytes | **borrowed / zero-copy** | raw `[u8]` | you want zero-copy reads (`Cow<[f64]>` straight from the buffer), or to (de)serialize a value from YAML/JSON (it derives `serde`) |
| [`froto_serde`](src/froto_serde.rs) | any `serde` type `T` ⇄ bytes | owned | **names, not ids** | you have a concrete Rust type and want it on the wire without building a `Value` — with no declared type, an id-less struct is written as a **keyvalue** (its field names), not a structure |
| [`froto_checked_value`](src/froto_checked_value.rs) | `arora_types::Value` ⇄ bytes, **type-directed** | owned | `Uuid` (declared) | you want the encoding *validated* against a runtime `ty::low::Type`, sharing the `arora_types::value_serde` walk with other backends (e.g. ROS 2 CDR) |

(`froto` = *from/to*: each converts one in-memory shape to and from the bytes.)
`froto_checked_value` does not change the format — it adds a type contract on top
of the same bytes `froto_value` writes; a test pins the two equal.

**Untyped vs. typed — keyvalue vs. structure.** `froto_serde` has no declared
type, so an id-less struct is written as a **keyvalue** (the map form above, keyed
by field name), exactly as `arora_types::value_serde` produces a `Value::KeyValue`
for the same struct. Minting ids by hashing names would be a false identity that
breaks the moment a field or struct is renamed. The **structure** form — real
type and field ids — is what the *typed* path writes: `froto_checked_value`,
driven by a declared `ty::low::Type` whose ids come from `#[derive(AroraType)]`.
Same rule as the value layer: **no ids ⇒ keyvalue; declared ids ⇒ structure.**

## C ABI

The reader, writer and allocator are exposed as a stable `#[no_mangle] extern "C"`
surface — the common interop point across binary formats, so a native C++ guest
(`arora-sdk/libs`) and a WASM guest can both drive the same buffers. The
reader/writer entry points are in [`ffi`](src/ffi/); the buffer allocator (for
WASM linear memory) is in [`alloc`](src/alloc.rs). The safe Rust API they wrap is
[`BufferWriter`](src/writer.rs) / [`BufferReader`](src/reader.rs).

## Modules

- [`format`](src/format.rs) — the type tags, alignment and size-prefix constants
  (the spec above, in code).
- [`writer`](src/writer.rs) / [`reader`](src/reader.rs) — the low-level
  `BufferWriter` / `BufferReader` cursor.
- [`ffi`](src/ffi/) / [`alloc`](src/alloc.rs) — the C ABI over them.
- [`froto_value`](src/froto_value.rs) / [`froto_borrowed_value`](src/froto_borrowed_value.rs) /
  [`froto_serde`](src/froto_serde.rs) / [`froto_checked_value`](src/froto_checked_value.rs) —
  the four (de)serializers above.
