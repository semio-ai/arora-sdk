//! Arora's geometry types over [`arora_types::value::Value`]: 2-, 3- and
//! 4-component vectors, quaternions and transforms, and their blending.
//!
//! Each type is a [`Value::Structure`] of `F32` fields, with a type id and
//! field ids this crate declares. This crate:
//!
//! - **builds** them: [`vec3`]`([f32; 3]) -> Value`, [`transform`]`(Transform)`, ...;
//! - **reads** them back into plain Rust types: [`as_vec3`]`(&Value) ->
//!   Option<[f32; 3]>`, [`as_transform`], ... Code that does geometry
//!   decodes a value once, computes on plain types, and encodes the result;
//! - **classifies** them: [`kind`];
//! - **blends** values: [`blend::blend_values`], which slerps quaternions and
//!   blends transforms part by part.
//!
//! Arithmetic needs none of this: a value's numbers read off its own
//! structure, whatever its type.

use arora_types::value::{Structure, StructureField, Value};
use uuid::Uuid;

pub mod blend;

// ---- type and field ids -------------------------------------------------------

/// The namespace of the geometry ids: the ASCII bytes of "vizij"
/// (`76 69 7a 69 6a`) in the leading bytes. The ids keep the namespace they
/// were first published under, so stored values and readers that hard-code
/// the ids, such as `@vizij/value-json`, keep reading them.
pub const NAMESPACE: u128 = 0x7669_7a69_6a00_0000_0000_0000_0000_0000;

const fn id(offset: u128) -> Uuid {
    Uuid::from_u128(NAMESPACE | offset)
}

/// Type id of the `vec2` structure.
pub const VEC2_TYPE: Uuid = id(0x0002);
/// Field ids of the `vec2` structure, in `[x, y]` order.
pub const VEC2_FIELDS: [Uuid; 2] = [id(0x0002_0001), id(0x0002_0002)];

/// Type id of the `vec3` structure.
pub const VEC3_TYPE: Uuid = id(0x0003);
/// Field ids of the `vec3` structure, in `[x, y, z]` order.
pub const VEC3_FIELDS: [Uuid; 3] = [id(0x0003_0001), id(0x0003_0002), id(0x0003_0003)];

/// Type id of the `vec4` structure.
pub const VEC4_TYPE: Uuid = id(0x0004);
/// Field ids of the `vec4` structure, in `[x, y, z, w]` order.
pub const VEC4_FIELDS: [Uuid; 4] = [
    id(0x0004_0001),
    id(0x0004_0002),
    id(0x0004_0003),
    id(0x0004_0004),
];

/// Type id of the `quat` structure (`[x, y, z, w]`).
pub const QUAT_TYPE: Uuid = id(0x0010);
/// Field ids of the `quat` structure, in `[x, y, z, w]` order.
pub const QUAT_FIELDS: [Uuid; 4] = [
    id(0x0010_0001),
    id(0x0010_0002),
    id(0x0010_0003),
    id(0x0010_0004),
];

/// Type id of the `transform` structure.
pub const TRANSFORM_TYPE: Uuid = id(0x0030);
/// Field id of the `transform` translation (a `vec3` structure).
pub const TRANSFORM_TRANSLATION: Uuid = id(0x0030_0001);
/// Field id of the `transform` rotation (a `quat` structure).
pub const TRANSFORM_ROTATION: Uuid = id(0x0030_0002);
/// Field id of the `transform` scale (a `vec3` structure).
pub const TRANSFORM_SCALE: Uuid = id(0x0030_0003);

// ---- plain forms and kinds ----------------------------------------------------

/// Plain-Rust form of the `transform` structure.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    /// Translation as `[x, y, z]`.
    pub translation: [f32; 3],
    /// Rotation quaternion as `[x, y, z, w]`.
    pub rotation: [f32; 4],
    /// Per-axis scale as `[x, y, z]`.
    pub scale: [f32; 3],
}

impl Transform {
    /// No translation, no rotation, unit scale.
    pub const IDENTITY: Transform = Transform {
        translation: [0.0; 3],
        rotation: [0.0, 0.0, 0.0, 1.0],
        scale: [1.0; 3],
    };
}

/// The geometry type of a value.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    /// `vec2` structure.
    Vec2,
    /// `vec3` structure.
    Vec3,
    /// `vec4` structure.
    Vec4,
    /// `quat` structure.
    Quat,
    /// `transform` structure.
    Transform,
}

impl Kind {
    /// The type's name: `vec2`, `vec3`, `vec4`, `quat` or `transform`.
    pub fn name(self) -> &'static str {
        match self {
            Kind::Vec2 => "vec2",
            Kind::Vec3 => "vec3",
            Kind::Vec4 => "vec4",
            Kind::Quat => "quat",
            Kind::Transform => "transform",
        }
    }
}

/// The geometry type `value` is a structure of, by its type id; `None` for
/// any other value.
pub fn kind(value: &Value) -> Option<Kind> {
    match value {
        Value::Structure(s) => match s.id {
            t if t == VEC2_TYPE => Some(Kind::Vec2),
            t if t == VEC3_TYPE => Some(Kind::Vec3),
            t if t == VEC4_TYPE => Some(Kind::Vec4),
            t if t == QUAT_TYPE => Some(Kind::Quat),
            t if t == TRANSFORM_TYPE => Some(Kind::Transform),
            _ => None,
        },
        _ => None,
    }
}

// ---- constructors -------------------------------------------------------------

/// 2D vector structure.
pub fn vec2(a: [f32; 2]) -> Value {
    float_structure(VEC2_TYPE, &VEC2_FIELDS, &a)
}

/// 3D vector structure.
pub fn vec3(a: [f32; 3]) -> Value {
    float_structure(VEC3_TYPE, &VEC3_FIELDS, &a)
}

/// 4D vector structure.
pub fn vec4(a: [f32; 4]) -> Value {
    float_structure(VEC4_TYPE, &VEC4_FIELDS, &a)
}

/// Quaternion structure (`[x, y, z, w]`).
pub fn quat(a: [f32; 4]) -> Value {
    float_structure(QUAT_TYPE, &QUAT_FIELDS, &a)
}

/// Transform structure (translation `vec3`, rotation `quat`, scale `vec3`).
pub fn transform(t: Transform) -> Value {
    structure(
        TRANSFORM_TYPE,
        vec![
            (TRANSFORM_TRANSLATION, vec3(t.translation)),
            (TRANSFORM_ROTATION, quat(t.rotation)),
            (TRANSFORM_SCALE, vec3(t.scale)),
        ],
    )
}

/// A structure of `F32` fields: type id `ty`, `fields[i]` holding `comps[i]`.
pub fn float_structure(ty: Uuid, fields: &[Uuid], comps: &[f32]) -> Value {
    structure(
        ty,
        fields
            .iter()
            .zip(comps)
            .map(|(id, c)| (*id, Value::F32(*c)))
            .collect(),
    )
}

fn structure(id: Uuid, fields: Vec<(Uuid, Value)>) -> Value {
    Value::Structure(Structure {
        id,
        fields: fields
            .into_iter()
            .map(|(id, value)| StructureField {
                id,
                value: Box::new(value),
            })
            .collect(),
    })
}

// ---- accessors ----------------------------------------------------------------

/// Read a `vec2` structure.
pub fn as_vec2(value: &Value) -> Option<[f32; 2]> {
    read_float_structure(value, VEC2_TYPE, &VEC2_FIELDS)
}

/// Read a `vec3` structure.
pub fn as_vec3(value: &Value) -> Option<[f32; 3]> {
    read_float_structure(value, VEC3_TYPE, &VEC3_FIELDS)
}

/// Read a `vec4` structure.
pub fn as_vec4(value: &Value) -> Option<[f32; 4]> {
    read_float_structure(value, VEC4_TYPE, &VEC4_FIELDS)
}

/// Read a `quat` structure (`[x, y, z, w]`).
pub fn as_quat(value: &Value) -> Option<[f32; 4]> {
    read_float_structure(value, QUAT_TYPE, &QUAT_FIELDS)
}

/// Read a `transform` structure into its plain form.
pub fn as_transform(value: &Value) -> Option<Transform> {
    let s = as_structure(value, TRANSFORM_TYPE)?;
    Some(Transform {
        translation: as_vec3(read_field(s, TRANSFORM_TRANSLATION)?)?,
        rotation: as_quat(read_field(s, TRANSFORM_ROTATION)?)?,
        scale: as_vec3(read_field(s, TRANSFORM_SCALE)?)?,
    })
}

/// Read the structure of type id `ty` whose fields `fields` hold numbers,
/// as `f32`s in the order of `fields`. A field of any number type reads,
/// widened or narrowed to `f32`.
pub fn read_float_structure<const N: usize>(
    value: &Value,
    ty: Uuid,
    fields: &[Uuid; N],
) -> Option<[f32; N]> {
    let s = as_structure(value, ty)?;
    let mut out = [0.0f32; N];
    for (slot, field_id) in out.iter_mut().zip(fields) {
        *slot = number(read_field(s, *field_id)?)?;
    }
    Some(out)
}

fn as_structure(value: &Value, ty: Uuid) -> Option<&Structure> {
    match value {
        Value::Structure(s) if s.id == ty => Some(s),
        _ => None,
    }
}

fn read_field(s: &Structure, field: Uuid) -> Option<&Value> {
    s.fields
        .iter()
        .find(|f| f.id == field)
        .map(|f| f.value.as_ref())
}

/// A number of any type, as an `f32`.
pub(crate) fn number(value: &Value) -> Option<f32> {
    match value {
        Value::F32(f) => Some(*f),
        Value::F64(f) => Some(*f as f32),
        Value::U8(n) => Some(*n as f32),
        Value::U16(n) => Some(*n as f32),
        Value::U32(n) => Some(*n as f32),
        Value::U64(n) => Some(*n as f32),
        Value::I8(n) => Some(*n as f32),
        Value::I16(n) => Some(*n as f32),
        Value::I32(n) => Some(*n as f32),
        Value::I64(n) => Some(*n as f32),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometry_round_trips() {
        assert_eq!(as_vec2(&vec2([1.0, 2.0])), Some([1.0, 2.0]));
        assert_eq!(as_vec3(&vec3([1.0, 2.0, 3.0])), Some([1.0, 2.0, 3.0]));
        assert_eq!(
            as_vec4(&vec4([1.0, 2.0, 3.0, 4.0])),
            Some([1.0, 2.0, 3.0, 4.0])
        );
        assert_eq!(
            as_quat(&quat([0.0, 0.0, 0.0, 1.0])),
            Some([0.0, 0.0, 0.0, 1.0])
        );
        let t = Transform {
            translation: [1.0, 2.0, 3.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0, 1.0, 1.0],
        };
        assert_eq!(as_transform(&transform(t)), Some(t));
    }

    #[test]
    fn accessors_read_fields_of_any_number_type() {
        let value = Value::Structure(Structure {
            id: VEC2_TYPE,
            fields: vec![
                StructureField {
                    id: VEC2_FIELDS[0],
                    value: Box::new(Value::F64(1.5)),
                },
                StructureField {
                    id: VEC2_FIELDS[1],
                    value: Box::new(Value::I32(-2)),
                },
            ],
        });
        assert_eq!(as_vec2(&value), Some([1.5, -2.0]));
    }

    #[test]
    fn accessors_reject_other_types() {
        assert_eq!(as_vec3(&vec4([1.0, 2.0, 3.0, 4.0])), None);
        assert_eq!(as_quat(&vec4([1.0, 2.0, 3.0, 4.0])), None);
        assert_eq!(as_vec3(&Value::ArrayF32(vec![1.0, 2.0, 3.0])), None);
        assert_eq!(as_transform(&vec3([0.0; 3])), None);
    }

    #[test]
    fn kinds_classify_by_type_id() {
        assert_eq!(kind(&vec2([0.0; 2])), Some(Kind::Vec2));
        assert_eq!(kind(&vec3([0.0; 3])), Some(Kind::Vec3));
        assert_eq!(kind(&vec4([0.0; 4])), Some(Kind::Vec4));
        assert_eq!(kind(&quat([0.0, 0.0, 0.0, 1.0])), Some(Kind::Quat));
        assert_eq!(kind(&transform(Transform::IDENTITY)), Some(Kind::Transform));
        assert_eq!(kind(&Value::F32(1.0)), None);
        let foreign = Value::Structure(Structure {
            id: Uuid::from_u128(0x9999),
            fields: vec![],
        });
        assert_eq!(kind(&foreign), None);
        assert_eq!(Kind::Quat.name(), "quat");
    }

    /// Pins the JSON wire form and the type-id strings that
    /// `@vizij/value-json` (npm) hard-codes in its arora-serde decoder.
    #[test]
    fn json_wire_form_matches_the_js_decoder() {
        assert_eq!(
            VEC2_TYPE.to_string(),
            "76697a69-6a00-0000-0000-000000000002"
        );
        assert_eq!(
            VEC3_TYPE.to_string(),
            "76697a69-6a00-0000-0000-000000000003"
        );
        assert_eq!(
            VEC4_TYPE.to_string(),
            "76697a69-6a00-0000-0000-000000000004"
        );
        assert_eq!(
            QUAT_TYPE.to_string(),
            "76697a69-6a00-0000-0000-000000000010"
        );
        assert_eq!(
            TRANSFORM_TYPE.to_string(),
            "76697a69-6a00-0000-0000-000000000030"
        );
        assert_eq!(
            TRANSFORM_TRANSLATION.to_string(),
            "76697a69-6a00-0000-0000-000000300001"
        );
        assert_eq!(
            TRANSFORM_ROTATION.to_string(),
            "76697a69-6a00-0000-0000-000000300002"
        );
        assert_eq!(
            TRANSFORM_SCALE.to_string(),
            "76697a69-6a00-0000-0000-000000300003"
        );
        assert_eq!(
            serde_json::to_string(&vec3([1.0, 2.0, 3.0])).unwrap(),
            r#"{"struct":{"id":"76697a69-6a00-0000-0000-000000000003","fields":[{"id":"76697a69-6a00-0000-0000-000000030001","value":{"f32":1.0}},{"id":"76697a69-6a00-0000-0000-000000030002","value":{"f32":2.0}},{"id":"76697a69-6a00-0000-0000-000000030003","value":{"f32":3.0}}]}}"#
        );
    }
}
