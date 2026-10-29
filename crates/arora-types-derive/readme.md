# arora-types-derive

`#[derive(AroraType)]` makes a Rust struct or enum an Arora type: its type
definition, its record, and its conversions to and from a `Value`, all from one
source — the Rust definition. Use it through
[`arora-types`](../arora-types/readme.md), which re-exports it under its default
`derive` feature: `use arora_types::AroraType;`.

## Ids are pinned

An Arora type is known by its id, and a structure's fields and an
enumeration's variants by theirs ([values](../arora-types/readme.md#values)).
Each one is pinned with `#[arora(id = "…")]`:

```rust
use arora_types::AroraType;

#[derive(AroraType)]
#[arora(id = "0a0a0a0a-0000-4000-8000-000000000001")]
struct Point {
    #[arora(id = "0a0a0a0a-0000-4000-8000-000000000002")]
    x: f64,
    #[arora(id = "0a0a0a0a-0000-4000-8000-000000000003")]
    y: f64,
}

#[derive(AroraType)]
#[arora(id = "0a0a0a0a-0000-4000-8000-000000000010")]
enum Mode {
    #[arora(id = "0a0a0a0a-0000-4000-8000-000000000011")]
    Idle,
    #[arora(id = "0a0a0a0a-0000-4000-8000-000000000012")]
    Running,
}
```

A missing id is a compile error. An id hashed from a name would change the
moment the type, field or variant is renamed, so it is no identity.

## What it generates

- **`AroraType`** — `arora_type_id()`; `arora_type()`, the `ty::low::Type`
  with fields or variants in declared order; `register_types()`, it and every
  type it names; `arora_type_version()`; `scope()`; `record()`, its structure
  or enumeration record; and `register_records()`, that record with the
  records of every type it names.
- **`From<T> for Value`** — a struct as a `Value::Structure` under the type's
  id, each field under its id in declared order; an enum as a
  `Value::Enumeration` under the variant's id.
- **`TryFrom<Value> for T`** — the inverse, failing with a `ConversionError`
  naming what did not match.

## Attributes

| Attribute | On | Means |
|---|---|---|
| `id = "<uuid>"` | type, field, variant | its id |
| `version = "major.minor.patch"` | type | the version of its record, which every record naming it pins; `1.0.0` when absent |
| `scope = <path to a Scope>` | type | the scope its record lives in, whatever names it ([scopes](../../docs/records.md#scopes)) |
| `name = "…"` | field, variant | its name in Arora, when that is not its Rust identifier |
| `name = "…"` | type | its name in Arora, and [name hashing](#name-hashing-for-ros-messages) for its ids |
| `keyvalue` | field | the field holds dynamically typed data: its type is the well-known `KeyValue`; a `Value` field crosses as it is, any other type through `value_serde`'s name-keyed conversion |

A raw identifier names the plain word: a field `r#type` is `type` in Arora.

## Supported shapes

- **Structs with named fields**, each field one of: a primitive (`bool`, the
  integers, `f32`, `f64`, `String`), `()`, `Uuid`, another `AroraType`, a `Box`
  of one (for a type that contains itself), a `Vec` of one (an array), an array
  `[T; N]` of one (a fixed-length array), an `Option` of one, or a
  `#[arora(keyvalue)]` field. An optional field reads a `Value::Option`, or a
  bare value as present; an absent optional field reads as `None`.
- **Enums with unit variants.**

Maps (`HashMap`, `BTreeMap`, `HashSet`), tuple and unit structs, unions, enum
variants carrying data, and a `Vec` or an `Option` as an array's element are
compile errors.

A struct holding a fixed-length array has a `ty::low::Type` but no record — a
record has no fixed-length form — so its `record()` is `None`, and a module
whose functions name it is refused by a registry.

## Name hashing for ROS messages

A ROS 2 message's qualified name (`geometry_msgs/msg/Point`) is its identity:
the ROS specification fixes it, not a Rust identifier. Such a type sets
`#[arora(name = "pkg/msg/Name")]` on the struct and no `id`; its id is then
hashed from that name, and each field's id from the field's name. This is how
[`arora-msgs-ros2`](../arora-msgs-ros2/README.md)'s generated messages agree with
the ROS type registry, and the one place name hashing fits: the name, not the
code, is the contract.

```rust
use arora_types::AroraType;

#[derive(AroraType)]
#[arora(name = "geometry_msgs/msg/Point")]
struct Point {
    x: f64,
    y: f64,
    z: f64,
}
```

## Records

`record(default_scope)` is the type's structure or enumeration record at its
version, each type it names pinned at that type's own version. It lives in the
scope the type states; a type stating none lives in `default_scope`, the scope
of what names it — the module whose function names it, or the type whose field
does — and at the root with neither. [`docs/records.md`](../../docs/records.md)
explains records and scopes.

## License

Apache-2.0.
