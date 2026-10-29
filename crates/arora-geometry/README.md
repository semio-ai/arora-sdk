# arora-geometry

Arora's geometry types over its values
([`arora_types::value::Value`](../arora-types/readme.md)): 2-, 3- and
4-component vectors, quaternions and transforms, and the blending of values.
Code that computes on positions and rotations reads them from the values a
behavior or a module passes, and gives its results back as values.

## A geometry value

Each type is a `Value::Structure` of `F32` fields, under a type id and field
ids this crate declares. Code that does geometry decodes a value once into
plain Rust types, computes on those, and encodes the result:

```rust
use arora_geometry::{as_transform, blend::blend_values, transform, Transform};

let from = transform(Transform::IDENTITY);
let to = transform(Transform {
    translation: [2.0, 0.0, 0.0],
    rotation: [0.0, 0.0, 0.0, 1.0],
    scale: [3.0; 3],
});
let halfway = as_transform(&blend_values(&from, &to, 0.5)).expect("a transform");
assert_eq!(halfway.translation, [1.0, 0.0, 0.0]);
assert_eq!(halfway.scale, [2.0; 3]);
```

Code that only does arithmetic needs none of this: a value's numbers read off
its own structure, whatever its type, as the
[dataflow nodes](../arora-dataflow-nodes/README.md) read them.

## Types

| Type | Constructor | Accessor | Fields, in order |
|---|---|---|---|
| `vec2` | `vec2([f32; 2])` | `as_vec2` | `x`, `y` |
| `vec3` | `vec3([f32; 3])` | `as_vec3` | `x`, `y`, `z` |
| `vec4` | `vec4([f32; 4])` | `as_vec4` | `x`, `y`, `z`, `w` |
| `quat` | `quat([f32; 4])` | `as_quat` | `x`, `y`, `z`, `w` |
| `transform` | `transform(Transform)` | `as_transform` | `translation` (a `vec3`), `rotation` (a `quat`), `scale` (a `vec3`) |

- An accessor reads a field of any number type, as an `f32`, and gives `None`
  for a value of another type.
- `kind(&Value) -> Option<Kind>` gives a value's geometry type by its type
  id; `Kind::name` its name.
- `float_structure` and `read_float_structure` build and read a structure of
  `F32` fields under any type id, for a crate declaring types of its own the
  same way, as [`arora-graphics-types`](../arora-graphics-types/README.md)
  does.
- The ids (`VEC3_TYPE`, `VEC3_FIELDS`, `TRANSFORM_ROTATION`, …) carry the
  ASCII bytes of "vizij" in their leading bytes (`NAMESPACE`), so that stored
  values and the readers that hard-code these ids, such as
  `@vizij/value-json`, read them.
- No record declares these types, so no registry holds them and no function
  signature names them: the dataflow functions take and return them as
  values of any type.

## Blending

`blend::blend_values(a, b, t)` gives the value a fraction `t` of the way from
`a` to `b`. Two values blend when they have the same form:

- numbers lerp. Two `F64`s blend in `f64` and give an `F64`; any other two
  numbers give an `F32`;
- quaternions slerp (`blend::slerp`), the shorter way round;
- any other two structures with the same type id and field ids blend field
  by field, each field keeping its type: two `F32` or two `F64` fields lerp,
  and any other numbers, such as integers, step. A `vec3` blends component by
  component, and a transform lerps its translation and scale and slerps its
  rotation;
- `ArrayF32`s blend element by element, a missing element reading as 0, and
  a number blends with every element of an `ArrayF32`;
- key-value records (`Value::KeyValue`) blend the fields both hold, by name,
  and keep the other fields of the side `t` is nearer (`a` below 0.5);
- sequences (`Value::ArrayValue`) blend item by item, and keep the extra
  items of the side `t` is nearer.

Anything else steps (`blend::step_blend`): booleans, text, enumerations, and
two values of different forms give `a` below 0.5 and `b` from 0.5.

## Development and testing

```bash
cargo test -p arora-geometry
```

## Related packages

- [`arora-graphics-types`](../arora-graphics-types/README.md): the RGBA
  colour, declared the same way.
- [`arora-dataflow-nodes`](../arora-dataflow-nodes/README.md): its geometry
  and kinematics nodes, and its selectors, read and build these types.
- [`vizij-api-core`](https://github.com/vizij-ai/vizij-rs/tree/main/crates/api/vizij-api-core):
  Vizij's value vocabulary, which re-exports these types and adds colour,
  shapes and typed paths.
