# Arora Types Derive

`#[derive(AroraType)]` — generate an arora
[`ty::low::Type`](https://github.com/semio-ai/arora-sdk/blob/main/crates/arora-types/src/ty/low.rs)
from a Rust `struct`, so the Rust definition is the single source of truth for the
type's schema instead of a hand-authored YAML record.

It is re-exported from [`arora-types`](../arora-types/readme.md) under the `derive`
feature, so you write `use arora_types::AroraType;`.

## What it generates

For a derived struct the macro implements the `AroraType` trait:

- `arora_type_id() -> Uuid` — the id this type is known by (and the id a field of
  this type is referenced by).
- `arora_type() -> ty::low::Type` — its full structure type: the fields in
  declared order, each with its id and a `TypeRef` to its type.
- `register_types(&mut TypeRegistry)` — inserts this type and every nested
  `AroraType` it depends on.

With that `Type` in hand, `arora_types::value_serde::to_value_seeded` turns a value
into a **`Structure`** carrying the declared ids — the *id-based* path (see the
[`value_serde` section](../arora-types/readme.md#serializing-a-value-value_serde)).
Plain serde, with no type, produces a name-keyed **`KeyValue`** instead. That is
the whole point of deriving `AroraType`: to give a type real, stable identity.

## Ids are explicit — no name hashing

Every id **must** be pinned with `#[arora(id = "…uuid…")]`, on the type *and* on
every field. There is no name-hash default: an id derived from a name silently
changes the moment the type or field is renamed, so it is not a reliable
identity. A missing id is a compile-time error.

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
```

## ROS types opt into name hashing

A ROS 2 message's qualified name (`geometry_msgs/msg/Point`) *is* its stable
identity — a spec, not a refactorable Rust ident. Such a type opts into
name-hashing by giving `#[arora(name = "pkg/msg/Name")]` on the struct (and no
`id`): the macro then hashes that qualified name for the type id and each field's
name for its field id. This is how `arora-msgs-ros2`'s generated messages agree
with the ROS type registry. It is the *only* place name-hashing is appropriate —
because the name, not the code, is the contract.

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

## Supported field types

Primitive scalars (`bool`, `u8..=u64`, `i8..=i64`, `f32`, `f64`, `String`), other
`#[derive(AroraType)]` structs (nested, registered transitively), and `Vec<T>` /
`[T; N]` of any of those (a homogeneous array). `Option`, maps and enums need a
`ty::low` model extension and are rejected for now.

## License

MIT.
