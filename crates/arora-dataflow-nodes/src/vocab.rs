//! The records, enumerations and plain values the node functions compute
//! on, over [`arora_types::value::Value`].
//!
//! - A **record** is a `Value::KeyValue` under [`RECORD_TYPE`], whose field
//!   ids derive from the key names, read back in key-name order.
//! - An **enumeration** is a `Value::Enumeration` under [`ENUM_TYPE`], whose
//!   variant is identified by [`variant_id`] of its name.
//! - Numbers, booleans, text, numeric vectors (`ArrayF32`) and sequences
//!   (`ArrayValue`) are Arora's own.
//!
//! The ids keep the namespace they were first published under (the ASCII
//! bytes of "vizij", [`arora_geometry::NAMESPACE`]), so stored values and
//! readers that hard-code them keep reading them.

use arora_types::gen_uuid_from_str;
use arora_types::keyvalue::{KeyValue, KeyValueField};
use arora_types::value::{Enumeration, Value};
use uuid::Uuid;

const fn id(offset: u128) -> Uuid {
    Uuid::from_u128(arora_geometry::NAMESPACE | offset)
}

/// Enumeration type id of an enumeration whose variant set is open
/// (variants identified by [`variant_id`] of their name).
pub const ENUM_TYPE: Uuid = id(0x0040);

/// KeyValue id of a record (an open string-keyed field set).
pub const RECORD_TYPE: Uuid = id(0x0050);

// ---- constructors ----------------------------------------------------------------

/// Scalar float.
pub fn float(v: f32) -> Value {
    Value::F32(v)
}

/// Boolean. (Named `bool_` because `bool` is a primitive type name.)
pub fn bool_(v: bool) -> Value {
    Value::Boolean(v)
}

/// UTF-8 text.
pub fn text(s: &str) -> Value {
    Value::String(s.to_string())
}

/// Variable-length numeric vector.
pub fn vector(xs: Vec<f32>) -> Value {
    Value::ArrayF32(xs)
}

/// String-keyed record. Field ids derive deterministically from the key names.
pub fn record<'a>(entries: impl IntoIterator<Item = (&'a str, Value)>) -> Value {
    let mut kv = KeyValue::new_with_id(RECORD_TYPE);
    for (key, value) in entries {
        kv.set_field(KeyValueField::new_with_id(
            key,
            gen_uuid_from_str(key),
            value,
        ));
    }
    Value::KeyValue(kv)
}

/// Heterogeneous sequence. Any declared array/list/tuple distinction lives in
/// the path's declared shape, not in the value.
pub fn array(items: Vec<Value>) -> Value {
    Value::ArrayValue(items)
}

/// Enumeration value of the open vizij enum type: the variant is identified
/// by [`variant_id`] of its name.
pub fn enumeration(variant: &str, payload: Value) -> Value {
    Value::Enumeration(Enumeration {
        id: ENUM_TYPE,
        variant_id: variant_id(variant),
        value: Box::new(payload),
    })
}

/// Deterministic variant id for an enum variant name. Readers compare the
/// `variant_id` of an [`as_enumeration`] result against `variant_id("name")`;
/// the name itself is not stored in the value.
pub fn variant_id(variant: &str) -> Uuid {
    gen_uuid_from_str(variant)
}

// ---- accessors --------------------------------------------------------------------

/// Read a scalar number as a float: `F32`, `F64` narrowed to `f32`, or any
/// integer widened — the store carries integers too (the runtime's `dt` in
/// nanoseconds, a frame's dimensions), and arithmetic on them is arithmetic.
pub fn as_float(value: &Value) -> Option<f32> {
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

/// Read a boolean.
pub fn as_bool(value: &Value) -> Option<bool> {
    match value {
        Value::Boolean(b) => Some(*b),
        _ => None,
    }
}

/// Read UTF-8 text.
pub fn as_text(value: &Value) -> Option<&str> {
    match value {
        Value::String(s) => Some(s),
        _ => None,
    }
}

/// Read a variable-length numeric vector.
pub fn as_vector(value: &Value) -> Option<&[f32]> {
    match value {
        Value::ArrayF32(xs) => Some(xs),
        _ => None,
    }
}

/// Read a record as `(name, value)` pairs, sorted by name for determinism
/// (the underlying `KeyValue` field map is unordered). Fields without a value
/// are skipped.
pub fn as_record(value: &Value) -> Option<Vec<(&str, &Value)>> {
    match value {
        Value::KeyValue(kv) => {
            let mut entries: Vec<(&str, &Value)> = kv
                .fields
                .iter()
                .filter_map(|(key, field)| {
                    field.value.as_deref().map(|value| (key.as_str(), value))
                })
                .collect();
            entries.sort_by_key(|(key, _)| *key);
            Some(entries)
        }
        _ => None,
    }
}

/// Read one field of a record by name: `None` when `value` is no record, or
/// when the record has no field `name` or that field holds no value. One map
/// lookup, where [`as_record`] lists and sorts every field.
pub fn record_field<'a>(value: &'a Value, name: &str) -> Option<&'a Value> {
    match value {
        Value::KeyValue(kv) => kv.fields.get(name)?.value.as_deref(),
        _ => None,
    }
}

/// Read a heterogeneous sequence.
pub fn as_array(value: &Value) -> Option<&[Value]> {
    match value {
        Value::ArrayValue(items) => Some(items),
        _ => None,
    }
}

/// Read an enumeration as `(variant_id, payload)`. Compare the variant id
/// against [`variant_id`] of the expected variant name.
pub fn as_enumeration(value: &Value) -> Option<(Uuid, &Value)> {
    match value {
        Value::Enumeration(e) => Some((e.variant_id, &e.value)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_round_trips_and_sorts_by_name() {
        let value = record([
            ("shoulder", float(0.4)),
            ("elbow", float(-1.2)),
            ("nested", record([("x", float(1.0))])),
        ]);
        let entries = as_record(&value).expect("record");
        let names: Vec<&str> = entries.iter().map(|(name, _)| *name).collect();
        assert_eq!(names, vec!["elbow", "nested", "shoulder"]);
        assert_eq!(as_float(entries[0].1), Some(-1.2));
        assert_eq!(record_field(&value, "shoulder"), Some(&float(0.4)));
        assert_eq!(record_field(&value, "missing"), None);
    }

    #[test]
    fn enumeration_round_trips_via_variant_id() {
        let value = enumeration("grasp", float(0.5));
        let (variant, payload) = as_enumeration(&value).expect("enumeration");
        assert_eq!(variant, variant_id("grasp"));
        assert_ne!(variant, variant_id("release"));
        assert_eq!(payload, &float(0.5));
    }

    /// The ids are the ones values were first stored under, in the
    /// namespace of the geometry types, so a stored record or enumeration
    /// and a reader that hard-codes them keep reading them.
    #[test]
    fn the_ids_are_the_stored_ones() {
        assert_eq!(
            RECORD_TYPE,
            Uuid::from_u128(0x7669_7a69_6a00_0000_0000_0000_0000_0050)
        );
        assert_eq!(
            ENUM_TYPE,
            Uuid::from_u128(0x7669_7a69_6a00_0000_0000_0000_0000_0040)
        );
    }
}
