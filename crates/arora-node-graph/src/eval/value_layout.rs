//! Helpers for flattening structured values into contiguous numeric buffers.
//!
//! This is the kernel seam for numeric node math: [`flatten_numeric`] reads
//! a [`Value`] whose leaves are numbers into a [`FlatValue`] — a plain
//! `Vec<f32>` plus a [`ValueLayout`], the value's own form — operators
//! compute on the flat data, and [`ValueLayout::reconstruct`] rebuilds a
//! value of that form from the result. The layout is read off the value
//! itself, so the numeric nodes work on any structure the same way, with no
//! list of value types.

use arora_shape::value as vocab;
use arora_shape::value::{Structure, StructureField};
use arora_shape::{Shape, Value};
use hashbrown::HashMap;
use std::cell::RefCell;
use std::sync::Arc;
use uuid::Uuid;

use super::shape_helpers::infer_shape;

/// Evaluated output captured alongside its inferred shape.
#[derive(Clone, Debug)]
pub struct PortValue {
    pub value: Value,
    pub shape: Shape,
}

impl PortValue {
    /// Construct a `PortValue`, inferring the [`Shape`] from the [`Value`].
    pub fn new(value: Value) -> Self {
        let shape = infer_shape(&value);
        PortValue { value, shape }
    }

    /// Construct a `PortValue` with an explicit [`Shape`], bypassing inference.
    pub fn with_shape(value: Value, shape: Shape) -> Self {
        PortValue { value, shape }
    }

    /// Overwrite the cached [`Shape`] while leaving the [`Value`] untouched.
    pub fn set_shape(&mut self, shape: Shape) {
        self.shape = shape;
    }
}

impl Default for PortValue {
    fn default() -> Self {
        PortValue::new(vocab::float(0.0))
    }
}

/// How a value whose leaves are numbers is laid out when flattened.
///
/// The layout is the value's own form, so flattening needs no list of value
/// types: a structure flattens field by field in its declaration order and
/// rebuilds with its id and field ids, a record in key-name order, a sequence
/// item by item. Two values combine point-wise when their layouts have the
/// same form ([`ValueLayout::same_form`]): a vec4, a quaternion and a colour
/// each hold four numbers, and do not combine.
///
/// Sequences flatten to a single `Array` layout: the wire value is one
/// sequence kind (`ArrayValue`), so any declared array/list/tuple distinction
/// lives in the path's [`Shape`], not here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValueLayout {
    /// A number on its own, in a record or in a sequence, rebuilt as `F32`,
    /// the graph's number type, whatever width it was read from.
    Scalar,
    /// A structure's number field, rebuilt in the field's own type: a
    /// structure keeps the field types it declares.
    Field(Number),
    /// An `ArrayF32` of this length.
    Vector(usize),
    /// A structure: its id, and its fields' ids and layouts in declaration
    /// order.
    Structure(Arc<StructureLayout>),
    /// A record's fields, in key-name order.
    Record(Vec<(String, ValueLayout)>),
    /// A sequence's items.
    Array(Vec<ValueLayout>),
}

/// The layout of a structure: its id, and its fields' ids and layouts in
/// declaration order.
///
/// A structure's layout is shared: the first value of a form builds it, and
/// every later value of that form reads its numbers through it, so
/// flattening a structure allocates nothing but its numbers.
#[derive(Debug, PartialEq, Eq)]
pub struct StructureLayout {
    pub id: Uuid,
    pub fields: Vec<(Uuid, ValueLayout)>,
    len: usize,
}

impl StructureLayout {
    /// Push `s`'s numbers onto `data` if `s` has this form, and say whether
    /// it did. On `false`, `data` may hold some of them.
    fn read(&self, s: &Structure, data: &mut Vec<f32>) -> bool {
        s.id == self.id
            && s.fields.len() == self.fields.len()
            && self
                .fields
                .iter()
                .zip(&s.fields)
                .all(|((id, layout), field)| *id == field.id && layout.read(&field.value, data))
    }
}

/// The forms of structure met so far, by structure id. A structure id has
/// few forms; the count bounds what an unusual stream of values can add.
const FORMS_PER_ID: usize = 16;

thread_local! {
    static STRUCTURES: RefCell<HashMap<Uuid, Vec<Arc<StructureLayout>>>> =
        RefCell::new(HashMap::new());
}

/// The type of a structure's number field.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Number {
    F32,
    F64,
    U8,
    U16,
    U32,
    U64,
    I8,
    I16,
    I32,
    I64,
}

impl Number {
    /// The type of `value` and its reading as an `f32`, for a number.
    fn read(value: &Value) -> Option<(Number, f32)> {
        Some(match value {
            Value::F32(x) => (Number::F32, *x),
            Value::F64(x) => (Number::F64, *x as f32),
            Value::U8(x) => (Number::U8, *x as f32),
            Value::U16(x) => (Number::U16, *x as f32),
            Value::U32(x) => (Number::U32, *x as f32),
            Value::U64(x) => (Number::U64, *x as f32),
            Value::I8(x) => (Number::I8, *x as f32),
            Value::I16(x) => (Number::I16, *x as f32),
            Value::I32(x) => (Number::I32, *x as f32),
            Value::I64(x) => (Number::I64, *x as f32),
            _ => return None,
        })
    }

    /// `x` as a value of this type. An integer type takes the nearest
    /// integer it holds, and `NaN` as zero.
    fn write(self, x: f32) -> Value {
        let n = x.round();
        match self {
            Number::F32 => Value::F32(x),
            Number::F64 => Value::F64(f64::from(x)),
            Number::U8 => Value::U8(n as u8),
            Number::U16 => Value::U16(n as u16),
            Number::U32 => Value::U32(n as u32),
            Number::U64 => Value::U64(n as u64),
            Number::I8 => Value::I8(n as i8),
            Number::I16 => Value::I16(n as i16),
            Number::I32 => Value::I32(n as i32),
            Number::I64 => Value::I64(n as i64),
        }
    }
}

/// Numeric data flattened into row-major storage with an associated layout description.
#[derive(Clone, Debug)]
pub struct FlatValue {
    pub layout: ValueLayout,
    pub data: Vec<f32>,
}

impl ValueLayout {
    /// Number of scalar slots required to store this layout.
    pub fn scalar_len(&self) -> usize {
        match self {
            ValueLayout::Scalar | ValueLayout::Field(_) => 1,
            ValueLayout::Vector(len) => *len,
            ValueLayout::Structure(structure) => structure.len,
            ValueLayout::Record(fields) => {
                fields.iter().map(|(_, layout)| layout.scalar_len()).sum()
            }
            ValueLayout::Array(items) => items.iter().map(|layout| layout.scalar_len()).sum(),
        }
    }

    fn is_scalar(&self) -> bool {
        matches!(self, ValueLayout::Scalar)
    }

    /// Whether values of the two layouts combine point-wise: the same
    /// structure ids and field ids, record keys and lengths, whatever the
    /// types of their numbers.
    pub fn same_form(&self, other: &ValueLayout) -> bool {
        match (self, other) {
            (
                ValueLayout::Scalar | ValueLayout::Field(_),
                ValueLayout::Scalar | ValueLayout::Field(_),
            ) => true,
            (ValueLayout::Vector(a), ValueLayout::Vector(b)) => a == b,
            (ValueLayout::Structure(a), ValueLayout::Structure(b)) => {
                Arc::ptr_eq(a, b)
                    || (a.id == b.id
                        && a.fields.len() == b.fields.len()
                        && a.fields
                            .iter()
                            .zip(&b.fields)
                            .all(|((ia, la), (ib, lb))| ia == ib && la.same_form(lb)))
            }
            (ValueLayout::Record(fa), ValueLayout::Record(fb)) => {
                fa.len() == fb.len()
                    && fa
                        .iter()
                        .zip(fb)
                        .all(|((ka, la), (kb, lb))| ka == kb && la.same_form(lb))
            }
            (ValueLayout::Array(a), ValueLayout::Array(b)) => {
                a.len() == b.len() && a.iter().zip(b).all(|(la, lb)| la.same_form(lb))
            }
            _ => false,
        }
    }

    /// Reconstruct a structured [`Value`] from flattened scalar data.
    /// Missing data reads as `NaN`.
    pub fn reconstruct(&self, data: &[f32]) -> Value {
        let at = |offset: usize| data.get(offset).copied().unwrap_or(f32::NAN);
        let rest = |offset: usize| data.get(offset..).unwrap_or(&[]);
        match self {
            ValueLayout::Scalar => Value::F32(at(0)),
            ValueLayout::Field(number) => number.write(at(0)),
            ValueLayout::Vector(len) => Value::ArrayF32((0..*len).map(at).collect()),
            ValueLayout::Structure(structure) => {
                let mut offset = 0usize;
                Value::Structure(Structure {
                    id: structure.id,
                    fields: structure
                        .fields
                        .iter()
                        .map(|(field, layout)| {
                            let value = layout.reconstruct(rest(offset));
                            offset += layout.scalar_len();
                            StructureField {
                                id: *field,
                                value: Box::new(value),
                            }
                        })
                        .collect(),
                })
            }
            ValueLayout::Record(fields) => {
                let mut offset = 0usize;
                vocab::record(fields.iter().map(|(key, layout)| {
                    let value = layout.reconstruct(rest(offset));
                    offset += layout.scalar_len();
                    (key.as_str(), value)
                }))
            }
            ValueLayout::Array(items) => {
                let mut offset = 0usize;
                Value::ArrayValue(
                    items
                        .iter()
                        .map(|layout| {
                            let value = layout.reconstruct(rest(offset));
                            offset += layout.scalar_len();
                            value
                        })
                        .collect(),
                )
            }
        }
    }

    pub fn fill_with(&self, value: f32) -> Value {
        let len = self.scalar_len();
        let data = vec![value; len];
        self.reconstruct(&data)
    }

    /// Push `value`'s numbers onto `data` if it has this layout, and say
    /// whether it did. On `false`, `data` may hold some of them.
    fn read(&self, value: &Value, data: &mut Vec<f32>) -> bool {
        match (self, value) {
            (ValueLayout::Scalar, value) => match Number::read(value) {
                Some((_, x)) => {
                    data.push(x);
                    true
                }
                None => false,
            },
            (ValueLayout::Record(fields), Value::KeyValue(_)) => vocab::as_record(value)
                .is_some_and(|entries| {
                    entries.len() == fields.len()
                        && fields
                            .iter()
                            .zip(entries)
                            .all(|((key, layout), (name, field))| {
                                key == name && layout.read(field, data)
                            })
                }),
            (ValueLayout::Array(items), Value::ArrayValue(values)) => {
                items.len() == values.len()
                    && items
                        .iter()
                        .zip(values)
                        .all(|(layout, item)| layout.read(item, data))
            }
            (ValueLayout::Field(number), value) => match Number::read(value) {
                Some((read, x)) if read == *number => {
                    data.push(x);
                    true
                }
                _ => false,
            },
            (ValueLayout::Vector(len), Value::ArrayF32(xs)) if xs.len() == *len => {
                data.extend_from_slice(xs);
                true
            }
            (ValueLayout::Structure(structure), Value::Structure(s)) => structure.read(s, data),
            _ => false,
        }
    }
}

/// Flatten a [`Value`] whose leaves are all numbers: a number, an
/// `ArrayF32`, or a structure, record or sequence of those. Any other value
/// (a boolean, text, an enumeration, a structure with a field that is none of
/// these) has no flat form.
pub fn flatten_numeric(value: &Value) -> Option<FlatValue> {
    // Room for a number, a numeric array, or a structure up to a transform's
    // ten numbers, so that reading them does not grow the buffer.
    let mut data = Vec::with_capacity(match value {
        Value::ArrayF32(xs) => xs.len(),
        Value::Structure(_) | Value::KeyValue(_) | Value::ArrayValue(_) => 16,
        _ => 1,
    });
    let layout = flatten_into(value, &mut data, false)?;
    Some(FlatValue { layout, data })
}

/// Push `value`'s numbers onto `data` and return its layout. `in_structure`
/// marks a structure's field, whose number keeps its type.
fn flatten_into(value: &Value, data: &mut Vec<f32>, in_structure: bool) -> Option<ValueLayout> {
    if let Some((number, x)) = Number::read(value) {
        data.push(x);
        return Some(if in_structure {
            ValueLayout::Field(number)
        } else {
            ValueLayout::Scalar
        });
    }
    match value {
        Value::ArrayF32(xs) => {
            data.extend_from_slice(xs);
            Some(ValueLayout::Vector(xs.len()))
        }
        Value::Structure(s) if !s.fields.is_empty() => {
            let start = data.len();
            let known = STRUCTURES.with(|forms| {
                forms.borrow().get(&s.id).and_then(|forms| {
                    forms
                        .iter()
                        .find(|form| {
                            data.truncate(start);
                            form.read(s, data)
                        })
                        .cloned()
                })
            });
            if let Some(form) = known {
                return Some(ValueLayout::Structure(form));
            }
            data.truncate(start);
            let mut fields = Vec::with_capacity(s.fields.len());
            for field in &s.fields {
                fields.push((field.id, flatten_into(&field.value, data, true)?));
            }
            let form = Arc::new(StructureLayout {
                id: s.id,
                fields,
                len: data.len() - start,
            });
            STRUCTURES.with(|forms| {
                let mut forms = forms.borrow_mut();
                let forms = forms.entry(s.id).or_default();
                if forms.len() < FORMS_PER_ID {
                    forms.push(form.clone());
                }
            });
            Some(ValueLayout::Structure(form))
        }
        Value::KeyValue(_) => {
            // `as_record` yields entries sorted by name, keeping the flat
            // ordering deterministic.
            let entries = vocab::as_record(value)?;
            let mut fields = Vec::with_capacity(entries.len());
            for (key, field) in entries {
                fields.push((key.to_string(), flatten_into(field, data, false)?));
            }
            Some(ValueLayout::Record(fields))
        }
        Value::ArrayValue(items) => {
            let mut layouts = Vec::with_capacity(items.len());
            for item in items {
                layouts.push(flatten_into(item, data, false)?);
            }
            Some(ValueLayout::Array(layouts))
        }
        _ => None,
    }
}

/// Align two flattened values for point-wise operations, broadcasting scalars when possible.
pub fn align_flattened(
    a: &FlatValue,
    b: &FlatValue,
) -> Result<(ValueLayout, Vec<f32>, Vec<f32>), ValueLayout> {
    if a.layout.same_form(&b.layout) {
        return Ok((a.layout.clone(), a.data.clone(), b.data.clone()));
    }
    if a.layout.is_scalar() {
        let layout = b.layout.clone();
        let len = layout.scalar_len();
        let repeated = vec![a.data.first().copied().unwrap_or(f32::NAN); len];
        return Ok((layout, repeated, b.data.clone()));
    }
    if b.layout.is_scalar() {
        let layout = a.layout.clone();
        let len = layout.scalar_len();
        let repeated = vec![b.data.first().copied().unwrap_or(f32::NAN); len];
        return Ok((layout, a.data.clone(), repeated));
    }

    let len_a = a.layout.scalar_len();
    let len_b = b.layout.scalar_len();
    if len_a >= len_b {
        Err(a.layout.clone())
    } else {
        Err(b.layout.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn structure(id: u128, fields: Vec<(u128, Value)>) -> Value {
        Value::Structure(Structure {
            id: Uuid::from_u128(id),
            fields: fields
                .into_iter()
                .map(|(field, value)| StructureField {
                    id: Uuid::from_u128(field),
                    value: Box::new(value),
                })
                .collect(),
        })
    }

    fn layout_of(value: &Value) -> Arc<StructureLayout> {
        match flatten_numeric(value).map(|flat| flat.layout) {
            Some(ValueLayout::Structure(form)) => form,
            other => panic!("expected a structure layout, got {other:?}"),
        }
    }

    #[test]
    fn structures_of_one_form_share_their_layout() {
        let a = structure(0xA0, vec![(1, Value::F32(1.0)), (2, Value::F32(2.0))]);
        let b = structure(0xA0, vec![(1, Value::F32(3.0)), (2, Value::F32(4.0))]);
        assert!(Arc::ptr_eq(&layout_of(&a), &layout_of(&b)));
    }

    #[test]
    fn a_structure_of_other_field_types_has_its_own_layout() {
        let narrow = structure(0xA1, vec![(1, Value::F32(1.0))]);
        let wide = structure(0xA1, vec![(1, Value::F64(1.0))]);
        assert!(!Arc::ptr_eq(&layout_of(&narrow), &layout_of(&wide)));
        // Each rebuilds in its own field types, whichever was met first.
        for value in [&narrow, &wide, &narrow] {
            let flat = flatten_numeric(value).expect("numeric");
            assert_eq!(&flat.layout.reconstruct(&flat.data), value);
        }
    }

    #[test]
    fn a_structure_holding_records_and_sequences_rebuilds_as_it_was() {
        let nested = |x: f32| {
            structure(
                0xA2,
                vec![
                    (
                        1,
                        vocab::record([("b", Value::F32(x)), ("a", Value::F32(2.0))]),
                    ),
                    (
                        2,
                        Value::ArrayValue(vec![Value::F32(3.0), Value::ArrayF32(vec![x])]),
                    ),
                ],
            )
        };
        let first = layout_of(&nested(1.0));
        assert!(Arc::ptr_eq(&first, &layout_of(&nested(5.0))));
        let flat = flatten_numeric(&nested(5.0)).expect("numeric");
        assert_eq!(flat.data, vec![2.0, 5.0, 3.0, 5.0]);
        assert_eq!(flat.layout.reconstruct(&flat.data), nested(5.0));
    }
}
