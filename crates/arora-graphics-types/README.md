# arora-graphics-types

Arora's graphics types over its values
([`arora_types::value::Value`](../arora-types/readme.md)): the RGBA colour,
for code that passes colours between behaviors, modules and renderers.

## A colour value

A colour, `color-rgba`, is a `Value::Structure` of four `F32` fields, `r`,
`g`, `b` and `a`, linear by convention, under a type id and field ids this
crate declares. It computes and blends as any structure of numbers does,
field by field, so building and reading it is all it needs:

```rust
use arora_geometry::blend::blend_values;
use arora_graphics_types::{as_color_rgba, color_rgba};

let blue = color_rgba([0.0, 0.0, 1.0, 1.0]);
let clear_orange = color_rgba([1.0, 0.5, 0.0, 0.0]);
let halfway = as_color_rgba(&blend_values(&blue, &clear_orange, 0.5)).expect("a colour");
assert_eq!(halfway, [0.5, 0.25, 0.5, 0.5]);
```

## Reference

| Type | Constructor | Accessor | Fields, in order |
|---|---|---|---|
| `color-rgba` | `color_rgba([f32; 4])` | `as_color_rgba` | `r`, `g`, `b`, `a` |

- The accessor reads a field of any number type, as an `f32`, and gives
  `None` for a value of another type: a `vec4` holds four numbers too, and
  is not a colour.
- `arora_geometry::blend::blend_values` blends two colours component by
  component.
- The ids (`COLOR_RGBA_TYPE`, `COLOR_RGBA_FIELDS`) are in
  `arora_geometry::NAMESPACE`, the ASCII bytes of "vizij" in their leading
  bytes, so that stored colours and the readers that hard-code these ids,
  such as `@vizij/value-json`, read them.
- No record declares the type, and `arora_geometry::kind` does not classify
  it: it is not a geometry type.

```bash
cargo test -p arora-graphics-types
```

## Related packages

- [`arora-geometry`](../arora-geometry/README.md): the geometry types, the
  namespace of these ids, and the blending of values.
- [`vizij-api-core`](https://github.com/vizij-ai/vizij-rs/tree/main/crates/api/vizij-api-core):
  Vizij's value vocabulary, which re-exports this colour.
