//! Numeric helper utilities shared across node evaluators.

use arora_shape::value as vocab;
use arora_shape::Value;

use super::value_layout::{align_flattened, flatten_numeric};

/// Apply `op` pairwise to two numeric values, broadcasting scalars when possible.
pub fn binary_numeric<F>(lhs: &Value, rhs: &Value, op: F) -> Value
where
    F: Fn(f32, f32) -> f32 + Copy,
{
    match (flatten_numeric(lhs), flatten_numeric(rhs)) {
        (Some(a), Some(b)) => match align_flattened(&a, &b) {
            Ok((layout, da, db)) => {
                let data: Vec<f32> = da.iter().zip(db.iter()).map(|(x, y)| op(*x, *y)).collect();
                layout.reconstruct(&data)
            }
            Err(layout) => layout.fill_with(f32::NAN),
        },
        (Some(a), None) => a.layout.fill_with(f32::NAN),
        (None, Some(b)) => b.layout.fill_with(f32::NAN),
        (None, None) => vocab::float(f32::NAN),
    }
}

/// Apply `op` to every component of `input`.
pub fn unary_numeric<F>(input: &Value, op: F) -> Value
where
    F: Fn(f32) -> f32 + Copy,
{
    match flatten_numeric(input) {
        Some(flat) => {
            let data: Vec<f32> = flat.data.iter().map(|x| op(*x)).collect();
            flat.layout.reconstruct(&data)
        }
        None => vocab::float(f32::NAN),
    }
}

/// Read a [`Value`] as one number.
///
/// A number reads as itself and a boolean as `1` or `0`. A value holding
/// others reads as its first: a sequence's first item, a structure's first
/// field in declaration order, a record's first field in key-name order, an
/// enumeration's payload. Anything else (text, unit) reads as `0`.
pub fn as_float(v: &Value) -> f32 {
    match v {
        Value::F32(x) => *x,
        Value::F64(x) => *x as f32,
        Value::U8(x) => *x as f32,
        Value::U16(x) => *x as f32,
        Value::U32(x) => *x as f32,
        Value::U64(x) => *x as f32,
        Value::I8(x) => *x as f32,
        Value::I16(x) => *x as f32,
        Value::I32(x) => *x as f32,
        Value::I64(x) => *x as f32,
        Value::Boolean(b) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        Value::ArrayF32(xs) => xs.first().copied().unwrap_or(0.0),
        Value::ArrayF64(xs) => xs.first().map(|x| *x as f32).unwrap_or(0.0),
        Value::ArrayValue(items) => items.first().map(as_float).unwrap_or(0.0),
        Value::KeyValue(_) => vocab::as_record(v)
            .and_then(|entries| entries.first().map(|(_, value)| as_float(value)))
            .unwrap_or(0.0),
        Value::Enumeration(e) => as_float(&e.value),
        Value::Option(Some(inner)) => as_float(inner),
        Value::Structure(s) => s.fields.first().map(|f| as_float(&f.value)).unwrap_or(0.0),
        _ => 0.0,
    }
}

/// Read every number a [`Value`] holds, in order.
///
/// A number is one, a boolean `1` or `0`; a numeric array holds its items. A
/// sequence holds its items' numbers, a structure its fields' in declaration
/// order, a record its fields' in key-name order, an enumeration its
/// payload's. Text and unit hold none.
pub fn as_vector(v: &Value) -> Vec<f32> {
    let mut out = Vec::new();
    push_numbers(v, &mut out);
    out
}

fn push_numbers(v: &Value, out: &mut Vec<f32>) {
    match v {
        Value::ArrayF32(xs) => out.extend_from_slice(xs),
        Value::ArrayF64(xs) => out.extend(xs.iter().map(|x| *x as f32)),
        Value::ArrayU8(xs) => out.extend(xs.iter().map(|x| *x as f32)),
        Value::ArrayU16(xs) => out.extend(xs.iter().map(|x| *x as f32)),
        Value::ArrayU32(xs) => out.extend(xs.iter().map(|x| *x as f32)),
        Value::ArrayU64(xs) => out.extend(xs.iter().map(|x| *x as f32)),
        Value::ArrayI8(xs) => out.extend(xs.iter().map(|x| *x as f32)),
        Value::ArrayI16(xs) => out.extend(xs.iter().map(|x| *x as f32)),
        Value::ArrayI32(xs) => out.extend(xs.iter().map(|x| *x as f32)),
        Value::ArrayI64(xs) => out.extend(xs.iter().map(|x| *x as f32)),
        Value::ArrayBoolean(xs) => out.extend(xs.iter().map(|b| if *b { 1.0 } else { 0.0 })),
        Value::ArrayValue(items) => items.iter().for_each(|item| push_numbers(item, out)),
        Value::Structure(s) => s.fields.iter().for_each(|f| push_numbers(&f.value, out)),
        Value::KeyValue(_) => {
            for (_, value) in vocab::as_record(v).unwrap_or_default() {
                push_numbers(value, out);
            }
        }
        Value::Enumeration(e) => push_numbers(&e.value, out),
        Value::Option(Some(inner)) => push_numbers(inner, out),
        Value::F32(_)
        | Value::F64(_)
        | Value::U8(_)
        | Value::U16(_)
        | Value::U32(_)
        | Value::U64(_)
        | Value::I8(_)
        | Value::I16(_)
        | Value::I32(_)
        | Value::I64(_)
        | Value::Boolean(_) => out.push(as_float(v)),
        _ => {}
    }
}

/// Read a [`Value`] as a boolean: a boolean as itself, text as whether it is
/// non-empty, anything else as whether any number it holds is non-zero.
pub fn as_bool(v: &Value) -> bool {
    match v {
        Value::Boolean(b) => *b,
        Value::String(s) => !s.is_empty(),
        _ => as_vector(v).iter().any(|x| *x != 0.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pose() -> Value {
        vocab::transform(vocab::Transform {
            translation: [0.0, 0.0, 0.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0, 2.0, 3.0],
        })
    }

    #[test]
    fn a_structure_reads_as_its_first_field_and_holds_all_its_numbers() {
        assert_eq!(as_float(&pose()), 0.0);
        assert_eq!(
            as_vector(&pose()),
            vec![0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 2.0, 3.0]
        );
        // Its rotation and scale count, though its translation is zero.
        assert!(as_bool(&pose()));
    }

    #[test]
    fn a_record_holds_its_numbers_in_key_name_order() {
        let record = vocab::record([("b", Value::F32(2.0)), ("a", Value::Boolean(true))]);
        assert_eq!(as_vector(&record), vec![1.0, 2.0]);
        assert_eq!(as_float(&record), 1.0);
    }

    #[test]
    fn text_and_identifiers_hold_no_numbers() {
        for value in [
            Value::String("1".to_string()),
            Value::ArrayString(vec!["1".to_string()]),
            Value::Uuid(uuid::Uuid::nil()),
            Value::Unit,
        ] {
            assert!(as_vector(&value).is_empty(), "{value:?}");
            assert_eq!(as_float(&value), 0.0, "{value:?}");
        }
        assert!(as_bool(&Value::String("x".to_string())));
        assert!(!as_bool(&Value::String(String::new())));
    }
}
