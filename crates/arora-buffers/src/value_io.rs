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
    TYPE_BOOLEAN, TYPE_F32, TYPE_F64, TYPE_I16, TYPE_I32, TYPE_I64, TYPE_I8, TYPE_STRING,
    TYPE_STRUCTURE, TYPE_U16, TYPE_U32, TYPE_U64, TYPE_U8, TYPE_UNIT,
};

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
    fn read_struct_header(&mut self) -> Result<(Uuid, usize)> {
        self.expect_tag(TYPE_STRUCTURE, "struct")?;
        let (id, count) = self.inner.get_structure();
        Ok((Self::uuid_from(id)?, count as usize))
    }
    fn read_field_id(&mut self) -> Result<Uuid> {
        Self::uuid_from(self.inner.get_structure_field())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arora_types::value_serde::{read_value, write_value, TypeRegistry};
    use arora_types::module::low::TypeRef;
    use arora_types::ty::{self, low};
    use arora_types::value::{Structure, StructureField, Value};

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
}
