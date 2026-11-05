# Proposal 3: every reference resolves to a `Type`

Status: draft, for review.
Date: 2026-09-09.
Author: drafted by an LLM agent from the review thread of
[#217](https://github.com/semio-ai/arora-sdk/pull/217), for Victor's review.

Scope: the module-header vocabulary `arora_types::ty::low` and the walkers
built on it (validation, defaulting, the typed wire walk, the ROS 2 CDR
codec, the seeded serde bridge). The record vocabulary `record::ty` is a
separate axis (see
[design note](design_decisions.md#two-type-vocabularies-two-axes)) and is
not touched.

---

## 1. Context

`ty::low` says "what a position holds" in three ways.

**A `TypeRef`** is the declaration's vocabulary — a field, a parameter, a
return value:

```rust
enum TypeRef {
  Scalar { id },              // a type, by id
  Array { id },               // T[]
  FixedArray { id, len },     // T[N]
  Map { key_id, value_id },   // Map<K, V>
  Option { id },              // Option<T>
}
```

A reference is a nominal id, or a constructor applied to nominal ids. The
constructors take ids only, so a reference is one level deep: `Vec<Vec<u8>>`,
`Option<Vec<T>>` and `Vec<Option<T>>` have no spelling.

**A `Type`** is a definition: an id, a name, a description, and a kind —

```rust
enum TypeKind {
  Structure(Structure),
  Enumeration(Enumeration),
  Primitive(TypeRef),
}
```

— so a type is a structure, an enumeration, *or a reference it is made of*.
`ty::PRIMITIVE_TYPES` uses that last form for the fourteen scalar primitives:
`Type { id: U8_ID, kind: Primitive(Scalar { id: U8_ID }) }`. A bare-sequence
ROS message is the same form with an array reference. There is no `Type` for
any other compound shape: `u8[]`, `Point[3]`, `Option<T>`, `Map<K, V>` exist
only as references.

**A well-known id** names a shape without a definition: `U8_ID` … `STRING_ID`
for the scalars, but also `ARRAY_BOOLEAN_ID` … `ARRAY_STRING_ID`,
`ARRAY_VALUE_ID`, `OPTION_ID`, `KEY_VALUE_ID` for compound shapes.

### 1.1 The same shape, two spellings

`u8[]` is therefore both `TypeRef::Array { id: U8_ID }` — structural, what
`#[derive(AroraType)]`, the `.msg` code generation and the typed wire walk
produce and consume — and `TypeRef::Scalar { id: ARRAY_U8_ID }` — nominal,
what `Value::type_uuid()` reports for a `Value::ArrayU8` and what
`ty::low::validate` and `default_value` accept. Likewise `Option<T>` against
`OPTION_ID` and `Map<K, V>` against `KEY_VALUE_ID`.

The two spellings are only half reconciled. `validate_scalar_type_id` and
`default_value_for_scalar_type_id` accept `Scalar { ARRAY_U8_ID }`; the typed
wire walk does not — `write_by_ref` sees an id outside `PRIMITIVE_IDS`, looks
it up in the registry, and fails. A value's own `type_uuid()` is therefore not
something the walk can act on.

### 1.2 The symptoms

- **Every walker carries its own id ladder.** `if id == U8_ID … else if id ==
  ARRAY_U8_ID …` appears in `default_value_for_scalar_type_id`,
  `default_value_for_array_element_type_id`, `default_value_for_fixed_array`,
  `validate_scalar_type_id`, `validate_array_element_type_id`, the bridge's
  `declared_array`, the walk's `write_scalar`/`write_array`, and
  `arora::module_discovery`'s table. They agree today by care, not by
  construction.
- **`Value::type_uuid()` is not one function.** `ArrayU8` reports the array's
  id (`ARRAY_U8_ID`); `ArrayStructure { id: point, .. }` reports the
  *element's* id. A `Value` of `Point[]` and a `Value` of `Point` claim the
  same type. Nothing outside `value.rs` consumes `type_uuid()` today, which is
  why this has not bitten.
- **The seeded serde bridge needs a two-headed seed.** After #217 the seed's
  declaration is `Type(&low::Type) | Array(&TypeRef)`: a resolved type where
  the model has one, the reference itself where it does not. The enum is
  honest about the model, and it exists only because the model has no `Type`
  to hand back for an array position.
- **One level of nesting.** `element_id_for` in the derive rejects `Vec`,
  `Option`, `HashMap`, `BTreeMap`, `HashSet` and `Box` as array elements; the
  `.msg` parser has no nested sequences to express. This is the reference
  algebra's depth-1 limit surfacing in every front end.

## 2. Proposal

### 2.1 One function from a reference to a type

```rust
/// The type a reference denotes.
pub fn resolve<'a>(type_ref: &TypeRef, registry: &'a TypeRegistry) -> Option<Cow<'a, Type>>;
```

- `Scalar { id }` with a well-known id → the entry in the well-known table
  (borrowed, `'static`).
- `Scalar { id }` otherwise → the registry's `Type` (borrowed), or `None`.
- `Array`, `FixedArray`, `Map`, `Option` → the **anonymous type** of that
  shape: `Type { id: structural id, name: derived, kind: Primitive(type_ref) }`
  — borrowed from the well-known table when the shape is well known (the
  twelve typed arrays of scalars, `ArrayValue`, `Option`, `KeyValue`), owned
  otherwise.

`Type` gains nothing; `TypeKind::Primitive(TypeRef)` is already the
representation of "a type made of a reference". What is new is that the model
can *produce* one for any reference, not only for scalars.

### 2.2 Structural ids

An anonymous compound type's id is a function of its shape. The well-known
ids that exist today are those functions' values for the well-known shapes:

| shape                        | id                                   |
|------------------------------|--------------------------------------|
| `Array { id: U8_ID }`        | `ARRAY_U8_ID` (existing)             |
| `Array { id: STRING_ID }`    | `ARRAY_STRING_ID` (existing)         |
| `Option { id: _ }`           | `OPTION_ID` (existing) †             |
| `Map { .. }`                 | `KEY_VALUE_ID` (existing) †          |
| `Array { id: point }`        | derived from (`array`, point)        |
| `FixedArray { id, len }`     | derived from (`fixedarray`, id, len) |

† `OPTION_ID` and `KEY_VALUE_ID` name the constructor regardless of its
argument, as `Value::Option`/`Value::KeyValue` are untyped today. Whether
`Option<Point>` deserves its own structural id is an open question (§5); the
proposal keeps today's ids for today's shapes and adds nothing that
contradicts them.

The derived ids use UUID v5 (RFC 4122, SHA-1 over a fixed namespace and the
shape's canonical text), not `gen_uuid_from_str`: the latter hashes with
`std::collections::hash_map::DefaultHasher`, whose algorithm the standard
library does not promise across releases. Structural ids are recomputed on
every run by every party, so they must be stable by specification.
`gen_uuid_from_str` stays as it is for the ids already in the wild.

With this, `Scalar { ARRAY_U8_ID }` and `Array { U8_ID }` denote the same
`Type` by construction, and `Value::type_uuid()` becomes one function: every
array reports its array's structural id, `ArrayStructure { id: point }`
included.

### 2.3 The leaf rule

A well-known scalar's `Type` refers to itself: `Primitive(Scalar { id })` with
`id == ty.id`. A walker that resolves references must treat that as a leaf,
not resolve again. Every walker already stops at `PRIMITIVE_IDS`; the rule is
stated once, on `resolve`, and nothing else changes.

### 2.4 What each walker becomes

A walker dispatches on a `Type`:

```rust
match &ty.kind {
  Structure(s)    => …,          // fields: resolve each field's reference
  Enumeration(e)  => …,
  Primitive(r)    => match r {   // a leaf, or a compound with resolved element(s)
    Scalar { id }  => leaf(id),
    Array { id }   => elements(resolve(Scalar { id }))
    …
  },
}
```

The id ladders collapse into one place — the leaf — and the array/element
split stops being re-derived per walker. In the seeded serde bridge the seed
becomes `{ ty: Cow<Type>, registry }`; `child(type_ref)` is `resolve`; the
`Declaration` enum is deleted.

### 2.5 What does not change

- The wire. Every existing id keeps its value; the structural ids are new
  values for shapes that had no id.
- `TypeRef`'s serde form, `Type`'s serde form, `module.yaml`.
- The derive and the `.msg` code generation: they keep emitting
  `TypeRef::Array { id }` for `Vec<T>`; only the consumers gain a `Type` to
  resolve it to.
- `record::ty`.

## 3. What it opens

Once an anonymous compound type has a stable id, it can be *registered*, and
then referenced by that id like any nominal type. `Vec<Vec<u8>>` becomes
`Array { id: structural_id(Array { U8_ID }) }`, with the inner type in the
registry — the derive and the `.msg` parser register the inner shape the way
they register a nested struct today. No new `TypeRef` variant, no wire change;
the depth-1 limit lifts one shape at a time as front ends opt in. This is not
part of the proposal's first stages; it is the reason the structural ids are
worth getting right.

## 4. Staging

Each stage is its own PR and lands green on its own.

1. **`ty`: `resolve` and the well-known table.** Additive; minor bump of
   `arora-types`. Fixes `Value::type_uuid()` for `ArrayStructure` /
   `ArrayEnumeration` (no consumers — see §1.2). Golden tests pin every
   well-known structural id and the derivation of a non-well-known one.
2. **Bridge: the seed is a `Type`.** `Declaration` deleted; behaviour
   unchanged; the seeded tests from #217 are the regression net.
3. **Walkers dispatch on `Type`.** `validate`, `default_value`, the typed
   walk, the CDR codec, `module_discovery`: one leaf table. Behaviour
   unchanged; each walker's own tests are the net.
4. **Nested compounds** (§3), when a front end needs one.

## 5. Open questions

- **Naming.** `ty::resolve(&TypeRef, &TypeRegistry)` vs a method
  `TypeRef::resolve(&self, registry)`; `WELL_KNOWN_TYPES` (superset of
  `PRIMITIVE_TYPES`) vs extending `PRIMITIVE_TYPES` in place.
- **Names of anonymous types.** `"u8[]"`, `"Point[3]"`? The `.msg` spelling
  is the only precedent; the name matters for messages and editors only.
- **`Option<T>` and `Map<K, V>` ids.** Keep the constructor-only ids
  (`OPTION_ID`, `KEY_VALUE_ID`) as the structural ids for every argument, or
  derive per argument and keep the old ids as the untyped case? The typed
  walk supports neither today, so this can wait for stage 3.
- **`ArrayEnumeration`.** The bridge rejects it, the walk does not carry it;
  it gets a structural id in stage 1 for consistency and nothing else until
  someone needs it.

## 6. Alternatives considered

- **The seed holds references, resolving on use.** Uniform, but every
  position is then a lookup, including each element of a `u8[]`, and the root
  type must be in the registry (the seeded entry points take a `&Type` that
  need not be). Resolution belongs at the boundary, once.
- **Drop the nominal `ARRAY_*_ID`s and spell arrays structurally only.**
  Breaks `Value::type_uuid()` for every typed array and `module_discovery`'s
  table on the wire. The structural-id reading of the existing ids costs
  nothing and keeps them.
- **Inline definitions in `TypeRef`.** A reference that carries its
  definition can neither share nor recurse; the nominal id is the right
  reference, and `resolve` is the right place to pay for it.
- **An arena for anonymous `Type`s instead of `Cow`.** Avoids the owned case
  entirely, at the cost of threading an arena through every walker. `Cow`
  allocates one small `Type` per array-of-user-type *position*, never per
  element; the well-known shapes borrow. Revisit if profiling says so.
