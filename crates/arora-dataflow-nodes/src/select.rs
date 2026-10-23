//! Selectors: the part of a value a path of field names and indices names,
//! what the [`select`](crate::functions::SELECT) function applies to an
//! edge's value.

use std::borrow::Cow;

use arora_types::value::Value;
use serde::{Deserialize, Serialize};

use crate::vocab;

/// One step of a selector path applied to a structured output value.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum SelectorSegment {
    /// Traverse into a record/struct field by name.
    Field(String),
    /// Traverse into an array/vector/list position by zero-based index.
    Index(usize),
}

/// Selector path applied after reading an upstream output port.
pub type Selector = Vec<SelectorSegment>;

/// The part of `value` `selector` names, each segment applied to the part
/// the one before it selects.
///
/// The selection walks `value` by reference and clones only what it
/// selects: a field of a large record costs the field, not the record.
pub fn select(value: &Value, selector: &[SelectorSegment]) -> Result<Value, String> {
    let mut part = Cow::Borrowed(value);
    for segment in selector {
        part = match (segment, part) {
            (SelectorSegment::Field(field), Cow::Borrowed(value)) => select_field(value, field)?,
            (SelectorSegment::Index(index), Cow::Borrowed(value)) => select_index(value, *index)?,
            (SelectorSegment::Field(field), Cow::Owned(value)) => {
                Cow::Owned(select_field(&value, field)?.into_owned())
            }
            (SelectorSegment::Index(index), Cow::Owned(value)) => {
                Cow::Owned(select_index(&value, *index)?.into_owned())
            }
        };
    }
    Ok(part.into_owned())
}

/// The value field `field` of `value` selects: a record's field, a
/// transform's part, an enumeration's payload when `field` names its variant.
fn select_field<'a>(value: &'a Value, field: &str) -> Result<Cow<'a, Value>, String> {
    match value {
        Value::KeyValue(_) => vocab::record_field(value, field)
            .map(Cow::Borrowed)
            .ok_or_else(|| format!("selector field '{field}' missing in record")),
        Value::Structure(s) if s.id == arora_geometry::TRANSFORM_TYPE => transform_field_id(field)
            .and_then(|id| s.fields.iter().find(|f| f.id == id))
            .map(|f| Cow::Borrowed(f.value.as_ref()))
            .ok_or_else(|| format!("selector field '{field}' invalid for transform")),
        Value::Enumeration(e) => {
            if vocab::variant_id(field) == e.variant_id || e.variant_id.to_string() == *field {
                Ok(Cow::Borrowed(e.value.as_ref()))
            } else {
                Err(format!(
                    "selector field '{field}' does not match enum variant '{}'",
                    e.variant_id
                ))
            }
        }
        _ => Err(format!(
            "selector field '{field}' unsupported for {}",
            form(value)
        )),
    }
}

/// The value index `index` of `value` selects: an element of a numeric
/// vector, an item of a sequence, a structure's field in declaration order.
fn select_index(value: &Value, index: usize) -> Result<Cow<'_, Value>, String> {
    let out_of_bounds = || format!("selector index {index} out of bounds for {}", form(value));
    match value {
        Value::ArrayF32(items) => items
            .get(index)
            .map(|x| Cow::Owned(vocab::float(*x)))
            .ok_or_else(out_of_bounds),
        Value::ArrayValue(items) => items
            .get(index)
            .map(Cow::Borrowed)
            .ok_or_else(out_of_bounds),
        Value::Structure(s) => s
            .fields
            .get(index)
            .map(|f| Cow::Borrowed(f.value.as_ref()))
            .ok_or_else(out_of_bounds),
        _ => Err(format!(
            "selector index {index} unsupported for {}",
            form(value)
        )),
    }
}

/// The field of a transform `field` names.
fn transform_field_id(field: &str) -> Option<uuid::Uuid> {
    match field {
        "translation" | "position" => Some(arora_geometry::TRANSFORM_TRANSLATION),
        "rotation" => Some(arora_geometry::TRANSFORM_ROTATION),
        "scale" => Some(arora_geometry::TRANSFORM_SCALE),
        _ => None,
    }
}

/// What `value` is, as an error names it: its geometry type, else its form.
fn form(value: &Value) -> String {
    if let Some(kind) = arora_geometry::kind(value) {
        return kind.name().to_string();
    }
    match value {
        Value::Unit => "no value".to_string(),
        Value::Boolean(_) => "a boolean".to_string(),
        Value::String(_) => "text".to_string(),
        Value::ArrayF32(items) => format!("vector of len {}", items.len()),
        Value::ArrayValue(items) => format!("array of len {}", items.len()),
        Value::Structure(s) => format!("a structure of {} fields", s.fields.len()),
        Value::KeyValue(_) => "a record".to_string(),
        Value::Enumeration(_) => "an enumeration".to_string(),
        value if vocab::as_float(value).is_some() => "a number".to_string(),
        _ => "a value of another form".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(name: &str) -> SelectorSegment {
        SelectorSegment::Field(name.to_string())
    }

    /// A record's field, an array item, a vector's element and an
    /// enumeration's payload select, and no selector selects the whole.
    #[test]
    fn a_selector_names_a_part() {
        let value = vocab::record([
            (
                "values",
                vocab::record([("a", vocab::float(1.0)), ("b", vocab::text("x"))]),
            ),
            (
                "items",
                vocab::array(vec![
                    Value::ArrayF32(vec![1.0, 2.0, 3.0]),
                    vocab::float(4.0),
                ]),
            ),
            ("choice", vocab::enumeration("on", vocab::float(5.0))),
        ]);
        assert_eq!(
            select(&value, &[field("values"), field("a")]),
            Ok(vocab::float(1.0))
        );
        assert_eq!(
            select(&value, &[field("values"), field("b")]),
            Ok(vocab::text("x"))
        );
        assert_eq!(
            select(
                &value,
                &[
                    field("items"),
                    SelectorSegment::Index(0),
                    SelectorSegment::Index(1),
                ],
            ),
            Ok(vocab::float(2.0))
        );
        assert_eq!(
            select(&value, &[field("choice"), field("on")]),
            Ok(vocab::float(5.0))
        );
        assert_eq!(select(&value, &[]), Ok(value.clone()));
    }

    /// A missing field, an out-of-range index and a variant the value is not
    /// say what failed.
    #[test]
    fn a_failed_selection_says_why() {
        let value = vocab::record([
            ("v", arora_geometry::vec2([1.0, 2.0])),
            ("e", vocab::enumeration("on", vocab::float(1.0))),
        ]);
        let error = |selector: &[SelectorSegment]| select(&value, selector).unwrap_err();
        assert_eq!(
            error(&[field("missing")]),
            "selector field 'missing' missing in record"
        );
        assert_eq!(
            error(&[field("v"), SelectorSegment::Index(2)]),
            "selector index 2 out of bounds for vec2"
        );
        assert!(
            error(&[field("e"), field("off")]).starts_with("selector field 'off' does not match")
        );
        assert!(error(&[field("v"), field("x")]).starts_with("selector field 'x' unsupported"));
    }

    /// An index selects a structure's field in declaration order, whatever
    /// the structure's type, and a transform's parts select by name.
    #[test]
    fn an_index_selects_a_structure_field_and_a_name_a_transform_part() {
        use arora_types::value::{Structure, StructureField};
        let foreign = Value::Structure(Structure {
            id: uuid::Uuid::from_u128(0xF0),
            fields: vec![
                StructureField {
                    id: uuid::Uuid::from_u128(0xF1),
                    value: Box::new(Value::F64(1.5)),
                },
                StructureField {
                    id: uuid::Uuid::from_u128(0xF2),
                    value: Box::new(vocab::text("x")),
                },
            ],
        });
        assert_eq!(
            select(&foreign, &[SelectorSegment::Index(0)]),
            Ok(Value::F64(1.5))
        );
        assert_eq!(
            select(&foreign, &[SelectorSegment::Index(1)]),
            Ok(vocab::text("x"))
        );
        assert_eq!(
            select(&foreign, &[SelectorSegment::Index(2)]),
            Err("selector index 2 out of bounds for a structure of 2 fields".to_string())
        );
        assert_eq!(
            select(
                &arora_geometry::vec3([1.0, 2.0, 3.0]),
                &[SelectorSegment::Index(2)]
            ),
            Ok(vocab::float(3.0))
        );

        let transform = arora_geometry::transform(arora_geometry::Transform {
            translation: [1.0, 2.0, 3.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [4.0; 3],
        });
        assert_eq!(
            select(&transform, &[field("position")]),
            Ok(arora_geometry::vec3([1.0, 2.0, 3.0]))
        );
        assert_eq!(
            select(&transform, &[field("rotation"), SelectorSegment::Index(3)]),
            Ok(vocab::float(1.0))
        );
        assert_eq!(
            select(&transform, &[field("scale")]),
            Ok(arora_geometry::vec3([4.0; 3]))
        );
        assert_eq!(
            select(&transform, &[field("skew")]),
            Err("selector field 'skew' invalid for transform".to_string())
        );
    }
}
