//! Blending of [`Value`]s: the value a fraction `t` of the way from `a` to
//! `b`.
//!
//! Two values blend when they have the same form:
//! - numbers lerp. Two `F64`s blend in `f64` and give an `F64`; any other
//!   two numbers give an `F32`;
//! - quaternions slerp, the shorter way round;
//! - any other two structures with the same type id and field ids blend
//!   field by field, each field keeping its type: two `F32` or two `F64`
//!   fields lerp, and any other numbers, such as integers, step. A vec3
//!   blends component by component, and a transform lerps its translation
//!   and scale and slerps its rotation;
//! - `ArrayF32`s blend element by element, a missing element reading as 0,
//!   and a number blends with every element of an `ArrayF32`;
//! - records blend the fields both hold, by name, and keep the other fields
//!   of the side `t` is nearer (`a` below 0.5);
//! - sequences blend item by item, and keep the extra items of the side `t`
//!   is nearer.
//!
//! Anything else steps ([`step_blend`]): booleans, text, enumerations, and
//! two values of different forms give `a` below 0.5 and `b` from 0.5.

use arora_types::value::{Structure, StructureField, Value};

use crate::{as_quat, number, quat};

/// Linear interpolation for f32
#[inline]
fn lerp_f(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Normalize a quaternion represented as [x,y,z,w]
fn normalize_quat(q: [f32; 4]) -> [f32; 4] {
    let mag = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    if mag == 0.0 {
        [0.0, 0.0, 0.0, 1.0]
    } else {
        [q[0] / mag, q[1] / mag, q[2] / mag, q[3] / mag]
    }
}

/// Slerp between two unit quaternions q1, q2
pub fn slerp(q1: [f32; 4], q2: [f32; 4], t: f32) -> [f32; 4] {
    // Ensure inputs are normalized
    let qa = normalize_quat(q1);
    let mut qb = normalize_quat(q2);

    // Compute dot product
    let mut dot = qa[0] * qb[0] + qa[1] * qb[1] + qa[2] * qb[2] + qa[3] * qb[3];

    // If the dot product is negative, slerp won't take the short path.
    // Fix by reversing one quaternion.
    if dot < 0.0 {
        qb = [-qb[0], -qb[1], -qb[2], -qb[3]];
        dot = -dot;
    }

    // If quaternions are close, use lerp
    const DOT_THRESHOLD: f32 = 0.9995;
    if dot > DOT_THRESHOLD {
        let res = [
            lerp_f(qa[0], qb[0], t),
            lerp_f(qa[1], qb[1], t),
            lerp_f(qa[2], qb[2], t),
            lerp_f(qa[3], qb[3], t),
        ];
        return normalize_quat(res);
    }

    // Compute the angle between them and slerp
    let theta_0 = dot.clamp(-1.0, 1.0).acos(); // angle between input quaternions
    let theta = theta_0 * t;
    let sin_theta = theta.sin();
    let sin_theta_0 = theta_0.sin();

    let s0 = ((theta_0 - theta).sin()) / sin_theta_0;
    let s1 = sin_theta / sin_theta_0;

    [
        s0 * qa[0] + s1 * qb[0],
        s0 * qa[1] + s1 * qb[1],
        s0 * qa[2] + s1 * qb[2],
        s0 * qa[3] + s1 * qb[3],
    ]
}

/// Blend two generic vectors elementwise. If lengths differ, treat missing elements as 0.0.
fn blend_vector(a: &[f32], b: &[f32], t: f32) -> Vec<f32> {
    let n = std::cmp::max(a.len(), b.len());
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let ai = *a.get(i).unwrap_or(&0.0);
        let bi = *b.get(i).unwrap_or(&0.0);
        out.push(lerp_f(ai, bi, t));
    }
    out
}

/// The value a fraction `t` of the way from `a` to `b` (see the module
/// header).
pub fn blend_values(a: &Value, b: &Value, t: f32) -> Value {
    match (a, b) {
        (Value::F64(x), Value::F64(y)) => Value::F64(x + (y - x) * f64::from(t)),
        (Value::ArrayF32(va), Value::ArrayF32(vb)) => Value::ArrayF32(blend_vector(va, vb, t)),
        (Value::ArrayF32(va), _) if number(b).is_some() => {
            let vb = vec![number(b).unwrap_or(0.0); va.len()];
            Value::ArrayF32(blend_vector(va, &vb, t))
        }
        (_, Value::ArrayF32(vb)) if number(a).is_some() => {
            let va = vec![number(a).unwrap_or(0.0); vb.len()];
            Value::ArrayF32(blend_vector(&va, vb, t))
        }
        (Value::Structure(sa), Value::Structure(sb)) => {
            if let (Some(qa), Some(qb)) = (as_quat(a), as_quat(b)) {
                return quat(slerp(qa, qb, t));
            }
            blend_structures(sa, sb, t).unwrap_or_else(|| step_blend(a, b, t))
        }
        (Value::KeyValue(ra), Value::KeyValue(rb)) => {
            let (mut base, other) = if t < 0.5 {
                (ra.clone(), rb)
            } else {
                (rb.clone(), ra)
            };
            for (name, field) in base.fields.iter_mut() {
                let Some(base_value) = field.value.as_deref() else {
                    continue;
                };
                let Some(other_value) = other.fields.get(name).and_then(|f| f.value.as_deref())
                else {
                    continue;
                };
                // Keep a->b orientation regardless of which side is base.
                let blended = if t < 0.5 {
                    blend_values(base_value, other_value, t)
                } else {
                    blend_values(other_value, base_value, t)
                };
                field.value = Some(Box::new(blended));
            }
            Value::KeyValue(base)
        }
        (Value::ArrayValue(ia), Value::ArrayValue(ib)) => {
            Value::ArrayValue(blend_list_like(ia, ib, t))
        }
        _ => match (number(a), number(b)) {
            (Some(x), Some(y)) => Value::F32(lerp_f(x, y, t)),
            _ => step_blend(a, b, t),
        },
    }
}

/// Two structures of the same type id and field ids, blended field by
/// field; `None` for structures of different forms.
fn blend_structures(a: &Structure, b: &Structure, t: f32) -> Option<Value> {
    let same_fields = a.fields.len() == b.fields.len()
        && a.fields
            .iter()
            .zip(&b.fields)
            .all(|(fa, fb)| fa.id == fb.id);
    if a.id != b.id || !same_fields {
        return None;
    }
    Some(Value::Structure(Structure {
        id: a.id,
        fields: a
            .fields
            .iter()
            .zip(&b.fields)
            .map(|(fa, fb)| StructureField {
                id: fa.id,
                value: Box::new(blend_field(&fa.value, &fb.value, t)),
            })
            .collect(),
    }))
}

/// Two values of a structure's field, blended so the field keeps its type:
/// two `F32`s or two `F64`s lerp in their type, any other numbers step, and
/// anything else blends as [`blend_values`] does.
fn blend_field(a: &Value, b: &Value, t: f32) -> Value {
    match (a, b) {
        (Value::F32(x), Value::F32(y)) => Value::F32(lerp_f(*x, *y, t)),
        (Value::F64(_), Value::F64(_)) => blend_values(a, b, t),
        _ if number(a).is_some() || number(b).is_some() => step_blend(a, b, t),
        _ => blend_values(a, b, t),
    }
}

fn blend_list_like(a: &[Value], b: &[Value], t: f32) -> Vec<Value> {
    let len = a.len().max(b.len());
    let mut out = Vec::with_capacity(len);
    for idx in 0..len {
        match (a.get(idx), b.get(idx)) {
            (Some(va), Some(vb)) => out.push(blend_values(va, vb, t)),
            (Some(va), None) => {
                if t < 0.5 {
                    out.push(va.clone());
                }
            }
            (None, Some(vb)) => {
                if t >= 0.5 {
                    out.push(vb.clone());
                }
            }
            (None, None) => {}
        }
    }
    out
}

/// Step blending for step-only types: choose a for t < 0.5, else b.
pub fn step_blend(a: &Value, b: &Value, t: f32) -> Value {
    if t < 0.5 {
        a.clone()
    } else {
        b.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{as_transform, as_vec3, transform, vec3, Transform};
    use arora_types::keyvalue::{KeyValue, KeyValueField};
    use uuid::Uuid;

    fn record(entries: &[(&str, Value)]) -> Value {
        let mut kv = KeyValue::new_with_id(Uuid::from_u128(0x50));
        for (key, value) in entries {
            kv.set_field(KeyValueField::new_with_id(
                *key,
                Uuid::from_u128(key.len() as u128),
                value.clone(),
            ));
        }
        Value::KeyValue(kv)
    }

    fn field<'a>(record: &'a Value, name: &str) -> Option<&'a Value> {
        match record {
            Value::KeyValue(kv) => kv.fields.get(name)?.value.as_deref(),
            _ => None,
        }
    }

    #[test]
    fn blend_floats() {
        let r = blend_values(&Value::F32(0.0), &Value::F32(1.0), 0.5);
        assert_eq!(r, Value::F32(0.5));
        // Numbers of any type blend; two `F64`s stay `F64`.
        assert_eq!(
            blend_values(&Value::I32(0), &Value::F64(1.0), 0.25),
            Value::F32(0.25)
        );
        assert_eq!(
            blend_values(&Value::F64(0.0), &Value::F64(1.0), 0.25),
            Value::F64(0.25)
        );
    }

    #[test]
    fn blend_vec3() {
        let a = vec3([0.0, 0.0, 0.0]);
        let b = vec3([1.0, 2.0, 3.0]);
        let r = blend_values(&a, &b, 0.5);
        assert_eq!(as_vec3(&r), Some([0.5, 1.0, 1.5]));
    }

    #[test]
    fn blend_quats_slerp() {
        let a = quat([0.0, 0.0, 0.0, 1.0]);
        let b = quat([1.0, 0.0, 0.0, 0.0]);
        let r = blend_values(&a, &b, 0.5);
        let q = as_quat(&r).expect("quat");
        // Halfway between identity and a 180-degree X rotation: 90 degrees.
        let inv_sqrt2 = 1.0 / 2.0f32.sqrt();
        assert!((q[0] - inv_sqrt2).abs() < 1e-4);
        assert!((q[3] - inv_sqrt2).abs() < 1e-4);
    }

    #[test]
    fn blend_transform_trs() {
        let a = transform(Transform::IDENTITY);
        let b = transform(Transform {
            translation: [2.0, 0.0, 0.0],
            rotation: [1.0, 0.0, 0.0, 0.0],
            scale: [3.0; 3],
        });
        let r = as_transform(&blend_values(&a, &b, 0.5)).expect("transform");
        assert_eq!(r.translation, [1.0, 0.0, 0.0]);
        assert_eq!(r.scale, [2.0; 3]);
        // The rotation slerps: a quarter turn, not a normalised lerp of a
        // half-length quaternion.
        let inv_sqrt2 = 1.0 / 2.0f32.sqrt();
        assert!((r.rotation[0] - inv_sqrt2).abs() < 1e-4);
        assert!((r.rotation[3] - inv_sqrt2).abs() < 1e-4);
    }

    #[test]
    fn any_structure_blends_field_by_field_keeping_its_field_types() {
        let structure = |x: f64, y: f32| {
            Value::Structure(Structure {
                id: Uuid::from_u128(0xF0),
                fields: vec![
                    StructureField {
                        id: Uuid::from_u128(0xF1),
                        value: Box::new(Value::F64(x)),
                    },
                    StructureField {
                        id: Uuid::from_u128(0xF2),
                        value: Box::new(Value::F32(y)),
                    },
                ],
            })
        };
        assert_eq!(
            blend_values(&structure(0.0, 0.0), &structure(4.0, 8.0), 0.25),
            structure(1.0, 2.0)
        );
        // Structures of different forms step.
        let other = vec3([1.0, 2.0, 3.0]);
        assert_eq!(
            blend_values(&structure(0.0, 0.0), &other, 0.25),
            structure(0.0, 0.0)
        );
        assert_eq!(blend_values(&structure(0.0, 0.0), &other, 0.75), other);
    }

    #[test]
    fn a_structure_keeps_its_integer_fields_by_stepping_them() {
        let structure = |n: i32, x: f32| {
            Value::Structure(Structure {
                id: Uuid::from_u128(0xF0),
                fields: vec![
                    StructureField {
                        id: Uuid::from_u128(0xF1),
                        value: Box::new(Value::I32(n)),
                    },
                    StructureField {
                        id: Uuid::from_u128(0xF2),
                        value: Box::new(Value::F32(x)),
                    },
                ],
            })
        };
        let (a, b) = (structure(4, 0.0), structure(8, 4.0));
        assert_eq!(blend_values(&a, &b, 0.0), a);
        assert_eq!(blend_values(&a, &b, 0.25), structure(4, 1.0));
        assert_eq!(blend_values(&a, &b, 0.75), structure(8, 3.0));
    }

    #[test]
    fn blend_vector_mixed_lengths() {
        let a = Value::ArrayF32(vec![1.0, 2.0]);
        let b = Value::ArrayF32(vec![3.0, 4.0, 5.0]);
        let r = blend_values(&a, &b, 0.5);
        assert_eq!(r, Value::ArrayF32(vec![2.0, 3.0, 2.5]));
    }

    #[test]
    fn blend_vector_with_scalar_broadcasts() {
        let a = Value::F32(1.0);
        let b = Value::ArrayF32(vec![3.0, 5.0]);
        let r = blend_values(&a, &b, 0.5);
        assert_eq!(r, Value::ArrayF32(vec![2.0, 3.0]));
    }

    #[test]
    fn blend_records_by_field() {
        let a = record(&[("x", Value::F32(0.0)), ("only_a", Value::F32(7.0))]);
        let b = record(&[("x", Value::F32(1.0)), ("only_b", Value::F32(9.0))]);
        let low = blend_values(&a, &b, 0.25);
        assert_eq!(field(&low, "x"), Some(&Value::F32(0.25)));
        assert_eq!(field(&low, "only_a"), Some(&Value::F32(7.0)));
        assert_eq!(field(&low, "only_b"), None);

        let high = blend_values(&a, &b, 0.75);
        assert_eq!(field(&high, "x"), Some(&Value::F32(0.75)));
        assert_eq!(field(&high, "only_b"), Some(&Value::F32(9.0)));
        assert_eq!(field(&high, "only_a"), None);
    }

    #[test]
    fn blend_arrays_indexwise() {
        let a = Value::ArrayValue(vec![Value::F32(0.0), Value::F32(10.0)]);
        let b = Value::ArrayValue(vec![Value::F32(1.0)]);
        let r = blend_values(&a, &b, 0.25);
        assert_eq!(
            r,
            Value::ArrayValue(vec![Value::F32(0.25), Value::F32(10.0)])
        );
    }

    #[test]
    fn values_without_a_blend_step() {
        let (a, b) = (Value::Boolean(false), Value::Boolean(true));
        assert_eq!(blend_values(&a, &b, 0.25), a);
        assert_eq!(blend_values(&a, &b, 0.75), b);
        let (ta, tb) = (Value::String("left".into()), Value::String("right".into()));
        assert_eq!(blend_values(&ta, &tb, 0.9), tb);
        assert_eq!(step_blend(&ta, &tb, 0.25), ta);
        // A number and a vec3 have different forms.
        let v = vec3([1.0, 2.0, 3.0]);
        assert_eq!(blend_values(&Value::F32(0.0), &v, 0.75), v);
    }
}
