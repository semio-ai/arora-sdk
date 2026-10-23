# arora-geometry

> **Arora's geometry types over `arora_types::value::Value`: 2-, 3- and 4-component vectors, quaternions and transforms, and the blending of values.**

Each type is a `Value::Structure` of `F32` fields with a type id and field ids this crate declares. Code that does geometry decodes a value once into plain Rust types, computes on those, and encodes the result. Code that only does arithmetic needs none of this: a value's numbers read off its own structure, whatever its type.

## Types

| Type | Constructor | Accessor | Fields, in order |
|---|---|---|---|
| `vec2` | `vec2([f32; 2])` | `as_vec2` | `x`, `y` |
| `vec3` | `vec3([f32; 3])` | `as_vec3` | `x`, `y`, `z` |
| `vec4` | `vec4([f32; 4])` | `as_vec4` | `x`, `y`, `z`, `w` |
| `quat` | `quat([f32; 4])` | `as_quat` | `x`, `y`, `z`, `w` |
| `transform` | `transform(Transform)` | `as_transform` | `translation` (a `vec3`), `rotation` (a `quat`), `scale` (a `vec3`) |

- The ids (`VEC3_TYPE`, `VEC3_FIELDS`, `TRANSFORM_ROTATION`, ...) live under `NAMESPACE`, the ASCII bytes of "vizij" in the leading bytes. They keep the namespace they were first published under, so stored values and readers that hard-code them, such as `@vizij/value-json`, keep reading them.
- An accessor reads a field of any number type, as an `f32`, and gives `None` for a value of another type.
- `kind(&Value) -> Option<Kind>` gives a value's geometry type by its type id; `Kind::name` its name.
- `float_structure` and `read_float_structure` build and read a structure of `F32` fields under any type id, for a crate declaring types of its own the same way.

## Blending

`blend::blend_values(a, b, t)` gives the value a fraction `t` of the way from `a` to `b`. Two values blend when they have the same form:

- numbers lerp. Two `F64`s blend in `f64` and give an `F64`; any other two numbers give an `F32`;
- quaternions slerp, the shorter way round;
- any other two structures with the same type id and field ids blend field by field, each field keeping its type: two `F32` or two `F64` fields lerp, and any other numbers, such as integers, step. A vec3 blends component by component, and a transform lerps its translation and scale and slerps its rotation;
- `ArrayF32`s blend element by element, a missing element reading as 0, and a number blends with every element of an `ArrayF32`;
- records blend the fields both hold, by name, and keep the other fields of the side `t` is nearer (`a` below 0.5);
- sequences blend item by item, and keep the extra items of the side `t` is nearer.

Anything else steps (`blend::step_blend`): booleans, text, enumerations, and two values of different forms give `a` below 0.5 and `b` from 0.5.

## Example

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

## Development & Testing

```bash
cargo test -p arora-geometry
```

## Related Packages

- [`arora-dataflow-nodes`](../arora-dataflow-nodes/README.md): its geometry and kinematics nodes, and its edge selection, use these types.
- [`arora-shape`](../arora-shape/README.md): Vizij's vocabulary, which re-exports these types and adds colour, records and enumerations.
