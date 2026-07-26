//! arora-buffers as an [`arora_types::value_serde`] backend.
//!
//! Implements [`ValueWriter`]/[`ValueReader`] over [`BufferWriter`]/
//! [`BufferReader`], so the shared type-directed walk
//! ([`arora_types::value_serde::write_value`]/[`read_value`]) can (de)serialize
//! any [`Value`] against a runtime `ty::low::Type` — no generated Rust per type.
//! arora-buffers is self-describing, so the reader consumes each inline type tag
//! and **validates** it against the type the walk asks for.

use arora_types::value_serde::{Error, Result, ValueReader, ValueWriter};
use uuid::Uuid;

use crate::read::BufferReader;
use crate::write::BufferWriter;
use crate::{
    TYPE_ARRAY, TYPE_BOOLEAN, TYPE_F32, TYPE_F64, TYPE_I16, TYPE_I32, TYPE_I64, TYPE_I8,
    TYPE_STRING, TYPE_STRUCTURE, TYPE_U16, TYPE_U32, TYPE_U64, TYPE_U8, TYPE_UNIT,
};

/// Generates the buffers `write_*_array` methods for numeric/bool elements: the
/// element type tagged once at the array head, then the raw little-endian bulk.
/// Matches `serde_uuid`'s `Value` encoding so the walk and that path agree.
macro_rules! buffers_write_arrays {
    ($($method:ident($elem:ty) => ($tag:expr, $bulk:ident);)*) => {
        $(
            fn $method(&mut self, v: &[$elem]) -> Result<()> {
                self.inner.add_array_primitive($tag, v.len() as u32);
                self.inner.$bulk(v);
                Ok(())
            }
        )*
    };
}

/// The read counterpart of [`buffers_write_arrays`]: validate the element tag,
/// skip the single alignment, then read the raw elements one by one. A bulk
/// transmute would need an element-aligned, little-endian slice (UB otherwise)
/// — see the same reasoning on `serde_uuid`'s reader.
///
/// The walk owns its `Vec`s, so this per-element copy is at its floor (one
/// memcpy's worth). A future zero-copy borrowing accessor — returning `&[T]`
/// straight from the buffer for GPU / bulk consumers — is possible once the
/// buffer backing is guaranteed 8-aligned at its base (which would also retire
/// the `get_*_bulk` transmute's latent misalignment and let TS take a
/// `Float64Array` view). Deferred until such a consumer exists.
macro_rules! buffers_read_arrays {
    ($($method:ident($elem:ty) => ($tag:expr, $name:expr, $get:ident);)*) => {
        $(
            fn $method(&mut self) -> Result<Vec<$elem>> {
                let count = self.enter_scalar_array($tag, $name)?;
                self.inner.align();
                let mut items = Vec::with_capacity(count);
                for _ in 0..count {
                    items.push(self.inner.$get());
                }
                Ok(items)
            }
        )*
    };
}

/// Serialize a [`Value`] into an arora buffer via the shared walk.
pub struct BuffersValueWriter {
    inner: BufferWriter,
}

impl Default for BuffersValueWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl BuffersValueWriter {
    pub fn new() -> Self {
        Self {
            inner: BufferWriter::new(),
        }
    }

    /// Finish and return the size-prefixed buffer (ready for [`BuffersValueReader`]).
    pub fn finish(mut self) -> Box<[u8]> {
        self.inner.finalize()
    }
}

impl ValueWriter for BuffersValueWriter {
    fn write_unit(&mut self) -> Result<()> {
        self.inner.add_unit();
        Ok(())
    }
    fn write_bool(&mut self, v: bool) -> Result<()> {
        self.inner.add_boolean(v);
        Ok(())
    }
    fn write_u8(&mut self, v: u8) -> Result<()> {
        self.inner.add_u8(v);
        Ok(())
    }
    fn write_u16(&mut self, v: u16) -> Result<()> {
        self.inner.add_u16(v);
        Ok(())
    }
    fn write_u32(&mut self, v: u32) -> Result<()> {
        self.inner.add_u32(v);
        Ok(())
    }
    fn write_u64(&mut self, v: u64) -> Result<()> {
        self.inner.add_u64(v);
        Ok(())
    }
    fn write_i8(&mut self, v: i8) -> Result<()> {
        self.inner.add_i8(v);
        Ok(())
    }
    fn write_i16(&mut self, v: i16) -> Result<()> {
        self.inner.add_i16(v);
        Ok(())
    }
    fn write_i32(&mut self, v: i32) -> Result<()> {
        self.inner.add_i32(v);
        Ok(())
    }
    fn write_i64(&mut self, v: i64) -> Result<()> {
        self.inner.add_i64(v);
        Ok(())
    }
    fn write_f32(&mut self, v: f32) -> Result<()> {
        self.inner.add_f32(v);
        Ok(())
    }
    fn write_f64(&mut self, v: f64) -> Result<()> {
        self.inner.add_f64(v);
        Ok(())
    }
    fn write_string(&mut self, v: &str) -> Result<()> {
        self.inner.add_string(v);
        Ok(())
    }
    fn begin_struct(&mut self, id: Uuid, field_count: usize) -> Result<()> {
        self.inner
            .begin_structure(id.as_bytes(), field_count as u32);
        Ok(())
    }
    fn begin_field(&mut self, id: Uuid) -> Result<()> {
        self.inner.add_structure_field(id.as_bytes());
        Ok(())
    }
    buffers_write_arrays! {
        write_bool_array(bool) => (TYPE_BOOLEAN, add_boolean_raw_bulk);
        write_u8_array(u8) => (TYPE_U8, add_u8_raw_bulk);
        write_u16_array(u16) => (TYPE_U16, add_u16_raw_bulk);
        write_u32_array(u32) => (TYPE_U32, add_u32_raw_bulk);
        write_u64_array(u64) => (TYPE_U64, add_u64_raw_bulk);
        write_i8_array(i8) => (TYPE_I8, add_i8_raw_bulk);
        write_i16_array(i16) => (TYPE_I16, add_i16_raw_bulk);
        write_i32_array(i32) => (TYPE_I32, add_i32_raw_bulk);
        write_i64_array(i64) => (TYPE_I64, add_i64_raw_bulk);
        write_f32_array(f32) => (TYPE_F32, add_f32_raw_bulk);
        write_f64_array(f64) => (TYPE_F64, add_f64_raw_bulk);
    }
    // Strings are variable-width: tag once, then length-prefixed bytes with no
    // per-element tag and no alignment (matching `serde_uuid`'s reader).
    fn write_string_array(&mut self, v: &[String]) -> Result<()> {
        self.inner.add_array_primitive(TYPE_STRING, v.len() as u32);
        for s in v {
            self.inner.add_string_raw(s);
        }
        Ok(())
    }
    fn begin_struct_array(&mut self, element_id: Uuid, len: usize) -> Result<()> {
        self.inner
            .add_array_structure(element_id.as_bytes(), len as u32);
        Ok(())
    }
    fn begin_struct_element(&mut self, field_count: usize) -> Result<()> {
        // The element type is fixed by the array head, so a headerless struct
        // body: only the field count (no tag, no id), then the fields.
        self.inner.begin_structure_raw(field_count as u32);
        Ok(())
    }
}

/// Deserialize a [`Value`] from an arora buffer, validating each inline type tag
/// against the type the walk requests.
pub struct BuffersValueReader<'a> {
    inner: BufferReader<'a>,
}

impl<'a> BuffersValueReader<'a> {
    /// `buffer` is the size-prefixed buffer produced by [`BuffersValueWriter::finish`].
    pub fn new(buffer: &'a [u8]) -> Self {
        Self {
            inner: BufferReader::new(buffer),
        }
    }

    fn expect_tag(&mut self, expected: u8, name: &str) -> Result<()> {
        match self.inner.next_type() {
            Some(tag) if tag == expected => Ok(()),
            Some(tag) => Err(Error::new(format!(
                "expected {name} (tag {expected}), found tag {tag}"
            ))),
            None => Err(Error::new(format!(
                "expected {name} (tag {expected}), buffer ended"
            ))),
        }
    }

    fn uuid_from(bytes: &[u8]) -> Result<Uuid> {
        Uuid::from_slice(bytes).map_err(|e| Error::new(format!("invalid uuid bytes: {e}")))
    }

    /// Consumes an array head and validates its element tag, returning the count.
    fn enter_scalar_array(&mut self, expected_tag: u8, name: &str) -> Result<usize> {
        self.expect_tag(TYPE_ARRAY, "array")?;
        let (tag, count) = self.inner.get_array();
        if tag != expected_tag {
            return Err(Error::new(format!(
                "expected an array of {name} (element tag {expected_tag}), found element tag {tag}"
            )));
        }
        Ok(count as usize)
    }
}

impl ValueReader for BuffersValueReader<'_> {
    fn read_unit(&mut self) -> Result<()> {
        self.expect_tag(TYPE_UNIT, "unit")?;
        self.inner.get_unit();
        Ok(())
    }
    fn read_bool(&mut self) -> Result<bool> {
        self.expect_tag(TYPE_BOOLEAN, "bool")?;
        Ok(self.inner.get_boolean())
    }
    fn read_u8(&mut self) -> Result<u8> {
        self.expect_tag(TYPE_U8, "u8")?;
        Ok(self.inner.get_u8())
    }
    fn read_u16(&mut self) -> Result<u16> {
        self.expect_tag(TYPE_U16, "u16")?;
        Ok(self.inner.get_u16())
    }
    fn read_u32(&mut self) -> Result<u32> {
        self.expect_tag(TYPE_U32, "u32")?;
        Ok(self.inner.get_u32())
    }
    fn read_u64(&mut self) -> Result<u64> {
        self.expect_tag(TYPE_U64, "u64")?;
        Ok(self.inner.get_u64())
    }
    fn read_i8(&mut self) -> Result<i8> {
        self.expect_tag(TYPE_I8, "i8")?;
        Ok(self.inner.get_i8())
    }
    fn read_i16(&mut self) -> Result<i16> {
        self.expect_tag(TYPE_I16, "i16")?;
        Ok(self.inner.get_i16())
    }
    fn read_i32(&mut self) -> Result<i32> {
        self.expect_tag(TYPE_I32, "i32")?;
        Ok(self.inner.get_i32())
    }
    fn read_i64(&mut self) -> Result<i64> {
        self.expect_tag(TYPE_I64, "i64")?;
        Ok(self.inner.get_i64())
    }
    fn read_f32(&mut self) -> Result<f32> {
        self.expect_tag(TYPE_F32, "f32")?;
        Ok(self.inner.get_f32())
    }
    fn read_f64(&mut self) -> Result<f64> {
        self.expect_tag(TYPE_F64, "f64")?;
        Ok(self.inner.get_f64())
    }
    fn read_string(&mut self) -> Result<String> {
        self.expect_tag(TYPE_STRING, "string")?;
        Ok(self.inner.get_string().to_string())
    }
    fn enter_struct(&mut self, expected_id: Uuid, field_count: usize) -> Result<()> {
        self.expect_tag(TYPE_STRUCTURE, "struct")?;
        let (id, count) = self.inner.get_structure();
        let id = Self::uuid_from(id)?;
        if id != expected_id {
            return Err(Error::new(format!(
                "structure id {id} does not match expected type id {expected_id}"
            )));
        }
        if count as usize != field_count {
            return Err(Error::new(format!(
                "structure declares {count} fields, type expects {field_count}"
            )));
        }
        Ok(())
    }
    fn enter_field(&mut self, expected_id: Uuid) -> Result<()> {
        let id = Self::uuid_from(self.inner.get_structure_field())?;
        if id != expected_id {
            return Err(Error::new(format!(
                "field id {id} does not match expected {expected_id}"
            )));
        }
        Ok(())
    }
    buffers_read_arrays! {
        read_bool_array(bool) => (TYPE_BOOLEAN, "bool", get_boolean);
        read_u8_array(u8) => (TYPE_U8, "u8", get_u8);
        read_u16_array(u16) => (TYPE_U16, "u16", get_u16);
        read_u32_array(u32) => (TYPE_U32, "u32", get_u32);
        read_u64_array(u64) => (TYPE_U64, "u64", get_u64);
        read_i8_array(i8) => (TYPE_I8, "i8", get_i8);
        read_i16_array(i16) => (TYPE_I16, "i16", get_i16);
        read_i32_array(i32) => (TYPE_I32, "i32", get_i32);
        read_i64_array(i64) => (TYPE_I64, "i64", get_i64);
        read_f32_array(f32) => (TYPE_F32, "f32", get_f32);
        read_f64_array(f64) => (TYPE_F64, "f64", get_f64);
    }
    fn read_string_array(&mut self) -> Result<Vec<String>> {
        // Strings are variable-width and unaligned: no `align()`, no per-element
        // tag (matching the writer above and `serde_uuid`).
        let count = self.enter_scalar_array(TYPE_STRING, "string")?;
        let mut items = Vec::with_capacity(count);
        for _ in 0..count {
            items.push(self.inner.get_string().to_string());
        }
        Ok(items)
    }
    fn enter_struct_array(&mut self, element_id: Uuid) -> Result<usize> {
        self.expect_tag(TYPE_ARRAY, "array")?;
        let (tag, count) = self.inner.get_array();
        if tag != TYPE_STRUCTURE {
            return Err(Error::new(format!(
                "expected an array of structures (element tag {TYPE_STRUCTURE}), found element tag {tag}"
            )));
        }
        let id = Self::uuid_from(self.inner.get_uuid())?;
        if id != element_id {
            return Err(Error::new(format!(
                "array element type id {id} does not match expected {element_id}"
            )));
        }
        Ok(count as usize)
    }
    fn enter_struct_element(&mut self, field_count: usize) -> Result<()> {
        let count = self.inner.get_structure_raw();
        if count as usize != field_count {
            return Err(Error::new(format!(
                "struct array element declares {count} fields, type expects {field_count}"
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arora_types::module::low::TypeRef;
    use arora_types::ty::{self, low};
    use arora_types::value::{Structure, StructureField, StructureWithoutId, Value};
    use arora_types::value_serde::{read_value, write_value, TypeRegistry};

    fn id(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    fn field(name: &str, type_id: Uuid) -> low::StructureField {
        low::StructureField {
            name: name.to_string(),
            type_ref: TypeRef::Scalar { id: type_id },
        }
    }

    // Inner { a: i32, b: f32 } and Outer { inner: Inner, name: str, x: f64 }.
    // Declared field order (IndexMap) is a=first, b=second; inner, name, x.
    const INNER: u128 = 0x10;
    const OUTER: u128 = 0x20;

    fn inner_type() -> low::Type {
        let fields = [
            (id(0xA), field("a", *ty::I32_ID)),
            (id(0xB), field("b", *ty::F32_ID)),
        ]
        .into_iter()
        .collect();
        low::Type {
            name: "Inner".to_string(),
            id: id(INNER),
            description: String::new(),
            kind: low::TypeKind::Structure(low::Structure { fields }),
        }
    }

    fn outer_type() -> low::Type {
        let fields = [
            (id(0x1), field("inner", id(INNER))),
            (id(0x2), field("name", *ty::STRING_ID)),
            (id(0x3), field("x", *ty::F64_ID)),
        ]
        .into_iter()
        .collect();
        low::Type {
            name: "Outer".to_string(),
            id: id(OUTER),
            description: String::new(),
            kind: low::TypeKind::Structure(low::Structure { fields }),
        }
    }

    fn registry() -> TypeRegistry {
        let mut r = TypeRegistry::new();
        r.insert(id(INNER), inner_type());
        r.insert(id(OUTER), outer_type());
        r
    }

    fn vfield(field_id: u128, value: Value) -> StructureField {
        StructureField {
            id: id(field_id),
            value: Box::new(value),
        }
    }

    fn outer_value() -> Value {
        Value::Structure(Structure {
            id: id(OUTER),
            fields: vec![
                vfield(
                    0x1,
                    Value::Structure(Structure {
                        id: id(INNER),
                        fields: vec![vfield(0xA, Value::I32(7)), vfield(0xB, Value::F32(1.5))],
                    }),
                ),
                vfield(0x2, Value::String("hi".to_string())),
                vfield(0x3, Value::F64(2.0)),
            ],
        })
    }

    #[test]
    fn nested_struct_round_trips_through_ty_low() {
        let outer = outer_type();
        let registry = registry();
        let value = outer_value();

        let mut w = BuffersValueWriter::new();
        write_value(&outer, &registry, &value, &mut w).expect("write");
        let buf = w.finish();

        let mut r = BuffersValueReader::new(&buf);
        let back = read_value(&outer, &registry, &mut r).expect("read");
        assert_eq!(back, value);
    }

    #[test]
    fn value_fields_out_of_declared_order_are_rejected() {
        // Same fields, but name/x swapped relative to the type's declared order.
        let outer = outer_type();
        let registry = registry();
        let misordered = Value::Structure(Structure {
            id: id(OUTER),
            fields: vec![
                vfield(
                    0x1,
                    Value::Structure(Structure {
                        id: id(INNER),
                        fields: vec![vfield(0xA, Value::I32(7)), vfield(0xB, Value::F32(1.5))],
                    }),
                ),
                vfield(0x3, Value::F64(2.0)),
                vfield(0x2, Value::String("hi".to_string())),
            ],
        });
        let mut w = BuffersValueWriter::new();
        assert!(write_value(&outer, &registry, &misordered, &mut w).is_err());
    }

    #[test]
    fn a_value_not_matching_the_type_is_rejected() {
        let outer = outer_type();
        let registry = registry();
        // x declared f64, give it a string.
        let bad = Value::Structure(Structure {
            id: id(OUTER),
            fields: vec![
                vfield(
                    0x1,
                    Value::Structure(Structure {
                        id: id(INNER),
                        fields: vec![vfield(0xA, Value::I32(7)), vfield(0xB, Value::F32(1.5))],
                    }),
                ),
                vfield(0x2, Value::String("hi".to_string())),
                vfield(0x3, Value::String("not a double".to_string())),
            ],
        });
        let mut w = BuffersValueWriter::new();
        assert!(write_value(&outer, &registry, &bad, &mut w).is_err());
    }

    // Proves the derive obsoletes the hand-authored `inner_type()`/`outer_type()`
    // above: the same nested shape, its `ty::low::Type` and registry generated
    // from the Rust definition, round-trips through the arora-buffers walk.
    #[test]
    fn a_derived_type_drives_the_buffers_walk() {
        use arora_types::AroraType;

        #[derive(arora_types::AroraType)]
        struct Inner {
            a: i32,
            b: f32,
        }
        #[derive(arora_types::AroraType)]
        struct Outer {
            inner: Inner,
            name: String,
            x: f64,
        }

        let (ty, registry) = Outer::arora_type_with_registry();

        // A Value shaped by the derived type — field ids are the derived ids (a
        // hash of each field name), so no hand-authored Type is needed.
        let g = arora_types::gen_uuid_from_str;
        let sf = |field_id, value| StructureField {
            id: field_id,
            value: Box::new(value),
        };
        let value = Value::Structure(Structure {
            id: Outer::arora_type_id(),
            fields: vec![
                sf(
                    g("inner"),
                    Value::Structure(Structure {
                        id: Inner::arora_type_id(),
                        fields: vec![sf(g("a"), Value::I32(7)), sf(g("b"), Value::F32(1.5))],
                    }),
                ),
                sf(g("name"), Value::String("hi".to_string())),
                sf(g("x"), Value::F64(2.0)),
            ],
        });

        let mut w = BuffersValueWriter::new();
        write_value(&ty, &registry, &value, &mut w).expect("write");
        let buf = w.finish();
        let mut r = BuffersValueReader::new(&buf);
        let back = read_value(&ty, &registry, &mut r).expect("read");
        assert_eq!(back, value);
    }

    fn array_field(name: &str, element_id: Uuid) -> low::StructureField {
        low::StructureField {
            name: name.to_string(),
            type_ref: TypeRef::Array { id: element_id },
        }
    }

    // Point { x, y, z : f64 } and Shape { weights: f64[]; labels: string[];
    // flags: bool[]; points: Point[] } — a numeric, a string, a bool and a
    // struct array in one value.
    const POINT: u128 = 0x30;
    const SHAPE: u128 = 0x40;

    fn point_type() -> low::Type {
        let fields = [
            (id(0x31), field("x", *ty::F64_ID)),
            (id(0x32), field("y", *ty::F64_ID)),
            (id(0x33), field("z", *ty::F64_ID)),
        ]
        .into_iter()
        .collect();
        low::Type {
            name: "Point".to_string(),
            id: id(POINT),
            description: String::new(),
            kind: low::TypeKind::Structure(low::Structure { fields }),
        }
    }

    fn shape_type() -> low::Type {
        let fields = [
            (id(0x41), array_field("weights", *ty::F64_ID)),
            (id(0x42), array_field("labels", *ty::STRING_ID)),
            (id(0x43), array_field("flags", *ty::BOOLEAN_ID)),
            (id(0x44), array_field("points", id(POINT))),
        ]
        .into_iter()
        .collect();
        low::Type {
            name: "Shape".to_string(),
            id: id(SHAPE),
            description: String::new(),
            kind: low::TypeKind::Structure(low::Structure { fields }),
        }
    }

    fn shape_registry() -> TypeRegistry {
        let mut r = TypeRegistry::new();
        r.insert(id(POINT), point_type());
        r.insert(id(SHAPE), shape_type());
        r
    }

    fn point_element(x: f64, y: f64, z: f64) -> StructureWithoutId {
        StructureWithoutId {
            fields: vec![
                vfield(0x31, Value::F64(x)),
                vfield(0x32, Value::F64(y)),
                vfield(0x33, Value::F64(z)),
            ],
        }
    }

    fn shape_value(
        weights: Vec<f64>,
        labels: Vec<String>,
        flags: Vec<bool>,
        points: Vec<StructureWithoutId>,
    ) -> Value {
        // Fields in the type's declared order — the walk requires it.
        Value::Structure(Structure {
            id: id(SHAPE),
            fields: vec![
                vfield(0x41, Value::ArrayF64(weights)),
                vfield(0x42, Value::ArrayString(labels)),
                vfield(0x43, Value::ArrayBoolean(flags)),
                vfield(
                    0x44,
                    Value::ArrayStructure {
                        id: id(POINT),
                        elements: points,
                    },
                ),
            ],
        })
    }

    fn populated_shape() -> Value {
        shape_value(
            vec![1.5, -2.5, 3.75],
            vec!["a".to_string(), String::new(), "cee".to_string()],
            vec![true, false, true],
            vec![point_element(1.0, 2.0, 3.0), point_element(4.0, 5.0, 6.0)],
        )
    }

    #[test]
    fn arrays_round_trip_through_the_walk() {
        let shape = shape_type();
        let registry = shape_registry();
        let value = populated_shape();

        let mut w = BuffersValueWriter::new();
        write_value(&shape, &registry, &value, &mut w).expect("write");
        let buf = w.finish();
        let mut r = BuffersValueReader::new(&buf);
        assert_eq!(read_value(&shape, &registry, &mut r).expect("read"), value);
    }

    #[test]
    fn empty_arrays_round_trip_through_the_walk() {
        let shape = shape_type();
        let registry = shape_registry();
        let value = shape_value(vec![], vec![], vec![], vec![]);

        let mut w = BuffersValueWriter::new();
        write_value(&shape, &registry, &value, &mut w).expect("write");
        let buf = w.finish();
        let mut r = BuffersValueReader::new(&buf);
        assert_eq!(read_value(&shape, &registry, &mut r).expect("read"), value);
    }

    /// The unify proof: the walk's buffers backend and the registry-less
    /// `serde_uuid` `Value` path encode the same value — arrays and all — into
    /// byte-identical buffers, so a buffer written by one reads on the other.
    #[test]
    fn walk_and_serde_uuid_encode_identically() {
        let shape = shape_type();
        let registry = shape_registry();
        let value = populated_shape();

        let mut w = BuffersValueWriter::new();
        write_value(&shape, &registry, &value, &mut w).expect("write");
        let via_walk = w.finish();

        let via_serde_uuid = crate::serde_uuid::serialize(&value);
        assert_eq!(via_walk, via_serde_uuid, "walk and serde_uuid diverge");
        assert_eq!(crate::serde_uuid::deserialize(&via_walk), value);
    }
}
