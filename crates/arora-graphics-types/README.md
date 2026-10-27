# arora-graphics-types

> **Arora's graphics types over `arora_types::value::Value`: the RGBA colour.**

A colour is a `Value::Structure` of `F32` fields with a type id and field ids this crate declares. It computes and blends as any structure of numbers does, field by field, so nothing beyond building and reading it is needed.

## Types

| Type | Constructor | Accessor | Fields, in order |
|---|---|---|---|
| `color-rgba` | `color_rgba([f32; 4])` | `as_color_rgba` | `r`, `g`, `b`, `a`, linear by convention |

- The ids (`COLOR_RGBA_TYPE`, `COLOR_RGBA_FIELDS`) live under `arora_geometry::NAMESPACE`, the ASCII bytes of "vizij" in the leading bytes. They keep the namespace they were first published under, so stored colours and readers that hard-code them, such as `@vizij/value-json`, keep reading them.
- The accessor reads a field of any number type, as an `f32`, and gives `None` for a value of another type: a `vec4` holds four numbers too, and is not a colour.
- `arora_geometry::blend::blend_values` blends two colours component by component.

## Example

```rust
use arora_geometry::blend::blend_values;
use arora_graphics_types::{as_color_rgba, color_rgba};

let blue = color_rgba([0.0, 0.0, 1.0, 1.0]);
let clear_orange = color_rgba([1.0, 0.5, 0.0, 0.0]);
let halfway = as_color_rgba(&blend_values(&blue, &clear_orange, 0.5)).expect("a colour");
assert_eq!(halfway, [0.5, 0.25, 0.5, 0.5]);
```

## Development & Testing

```bash
cargo test -p arora-graphics-types
```

## Related Packages

- [`arora-geometry`](../arora-geometry/README.md): the geometry types, the namespace of these ids, and the blending of values.
- [`vizij-api-core`](https://github.com/vizij-ai/vizij-rs/tree/main/crates/api/vizij-api-core): Vizij's value vocabulary, which re-exports this colour.
