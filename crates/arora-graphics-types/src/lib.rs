//! Arora's graphics types over [`arora_types::value::Value`]: the RGBA
//! colour.
//!
//! `color-rgba` is a [`Value::Structure`] of four `F32` fields, `[r, g, b,
//! a]`, linear by convention, with a type id and field ids this crate
//! declares. [`color_rgba`] builds one and [`as_color_rgba`] reads it back.
//!
//! A colour needs nothing else: it computes and blends as any structure of
//! numbers does, field by field (`arora_geometry::blend::blend_values`).

use arora_geometry::{float_structure, read_float_structure, NAMESPACE};
use arora_types::value::Value;
use uuid::Uuid;

const fn id(offset: u128) -> Uuid {
    Uuid::from_u128(NAMESPACE | offset)
}

/// Type id of the `color-rgba` structure (linear by convention). It keeps
/// the id it was first published under, in the geometry types' namespace,
/// so stored colours and readers that hard-code it, such as
/// `@vizij/value-json`, keep reading them.
pub const COLOR_RGBA_TYPE: Uuid = id(0x0020);
/// Field ids of the `color-rgba` structure, in `[r, g, b, a]` order.
pub const COLOR_RGBA_FIELDS: [Uuid; 4] = [
    id(0x0020_0001),
    id(0x0020_0002),
    id(0x0020_0003),
    id(0x0020_0004),
];

/// RGBA colour structure (`[r, g, b, a]`, linear by convention).
pub fn color_rgba(a: [f32; 4]) -> Value {
    float_structure(COLOR_RGBA_TYPE, &COLOR_RGBA_FIELDS, &a)
}

/// Read a `color-rgba` structure (`[r, g, b, a]`). A field of any number
/// type reads, as an `f32`; a value of another type gives `None`.
pub fn as_color_rgba(value: &Value) -> Option<[f32; 4]> {
    read_float_structure(value, COLOR_RGBA_TYPE, &COLOR_RGBA_FIELDS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use arora_geometry::{blend::blend_values, vec4};
    use arora_types::value::{Structure, StructureField};

    #[test]
    fn a_colour_round_trips() {
        assert_eq!(
            as_color_rgba(&color_rgba([0.1, 0.2, 0.3, 1.0])),
            Some([0.1, 0.2, 0.3, 1.0])
        );
    }

    #[test]
    fn the_accessor_reads_fields_of_any_number_type_and_rejects_other_types() {
        let value = Value::Structure(Structure {
            id: COLOR_RGBA_TYPE,
            fields: COLOR_RGBA_FIELDS
                .iter()
                .zip([
                    Value::F64(0.5),
                    Value::U8(1),
                    Value::F32(0.0),
                    Value::I32(1),
                ])
                .map(|(id, value)| StructureField {
                    id: *id,
                    value: Box::new(value),
                })
                .collect(),
        });
        assert_eq!(as_color_rgba(&value), Some([0.5, 1.0, 0.0, 1.0]));
        assert_eq!(as_color_rgba(&vec4([0.1, 0.2, 0.3, 1.0])), None);
        assert_eq!(
            as_color_rgba(&Value::ArrayF32(vec![0.1, 0.2, 0.3, 1.0])),
            None
        );
    }

    #[test]
    fn colours_blend_component_by_component() {
        let halfway = blend_values(
            &color_rgba([0.0, 0.0, 1.0, 1.0]),
            &color_rgba([1.0, 0.5, 0.0, 0.0]),
            0.5,
        );
        assert_eq!(as_color_rgba(&halfway), Some([0.5, 0.25, 0.5, 0.5]));
    }

    /// Pins the ids and the JSON wire form that `@vizij/value-json` (npm)
    /// hard-codes in its arora-serde decoder.
    #[test]
    fn json_wire_form_matches_the_js_decoder() {
        assert_eq!(
            COLOR_RGBA_TYPE.to_string(),
            "76697a69-6a00-0000-0000-000000000020"
        );
        assert_eq!(
            serde_json::to_string(&color_rgba([0.0, 0.5, 1.0, 1.0])).unwrap(),
            r#"{"struct":{"id":"76697a69-6a00-0000-0000-000000000020","fields":[{"id":"76697a69-6a00-0000-0000-000000200001","value":{"f32":0.0}},{"id":"76697a69-6a00-0000-0000-000000200002","value":{"f32":0.5}},{"id":"76697a69-6a00-0000-0000-000000200003","value":{"f32":1.0}},{"id":"76697a69-6a00-0000-0000-000000200004","value":{"f32":1.0}}]}}"#
        );
    }
}
