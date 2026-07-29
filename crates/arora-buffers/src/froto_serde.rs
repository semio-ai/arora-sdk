//! Serde straight to the wire: any `Serialize`/`Deserialize` type reads and
//! writes arora buffers **without the [`Value`] intermediary** — the second
//! backend of serde-arora, next to [`arora_types::value_serde`]'s
//! `T ↔ Value`.
//!
//! The two backends encode identically: [`to_bytes`] produces exactly the
//! bytes [`crate::froto_value::serialize`] produces for
//! [`value_serde::to_value`] of the same data (a test pins it), so a payload
//! written by one side can be read by the other, or inspected as a `Value`
//! mid-way when introspection is worth the allocation. The mapping is the
//! same: structs and maps are keyvalues (sorted by key — a struct under the
//! hash of its type name, a map under [`value_serde::map_id`]), their fields
//! carrying their names, since an id-less struct has no honest type id to claim
//! (a name hash would change the moment a field or the struct is renamed);
//! enums are enumerations, sequences are value arrays.
//!
//! [`Value`]: arora_types::value::Value

use std::fmt::{self, Display};

use arora_types::gen_uuid_from_str;
use arora_types::value_serde;
use serde::de::{
    self, DeserializeOwned, DeserializeSeed, EnumAccess, IntoDeserializer, MapAccess, SeqAccess,
    VariantAccess, Visitor,
};
use serde::ser::{self, Serialize};

use crate::froto_value::{deserialize_from_reader, serialize_to_writer};
use crate::reader::BufferReader;
use crate::writer::BufferWriter;
use crate::{
    TYPE_ARRAY, TYPE_BOOLEAN, TYPE_ENUMERATION, TYPE_F32, TYPE_F64, TYPE_I16, TYPE_I32, TYPE_I64,
    TYPE_I8, TYPE_MAP, TYPE_OPTION, TYPE_STRING, TYPE_STRUCTURE, TYPE_U16, TYPE_U32, TYPE_U64,
    TYPE_U8, TYPE_UNIT, TYPE_UUID, TYPE_VALUE,
};

/// A conversion between a Rust type and the wire bytes failed.
#[derive(Debug)]
pub struct Error(String);

impl Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

impl ser::Error for Error {
    fn custom<T: Display>(msg: T) -> Self {
        Error(msg.to_string())
    }
}

impl de::Error for Error {
    fn custom<T: Display>(msg: T) -> Self {
        Error(msg.to_string())
    }
}

/// Serialize `value` straight into wire bytes, no `Value` built.
pub fn to_bytes<T: Serialize>(value: &T) -> Result<Box<[u8]>, Error> {
    let mut writer = BufferWriter::new();
    value.serialize(BytesSerializer {
        writer: &mut writer,
    })?;
    Ok(writer.finalize())
}

/// Deserialize a type straight out of wire bytes, no `Value` built.
pub fn from_bytes<T: DeserializeOwned>(data: &[u8]) -> Result<T, Error> {
    let mut reader = BufferReader::new(data);
    T::deserialize(BytesDeserializer {
        reader: &mut reader,
    })
}

// ---- serialization ---------------------------------------------------------

struct BytesSerializer<'w> {
    writer: &'w mut BufferWriter,
}

impl<'w> ser::Serializer for BytesSerializer<'w> {
    type Ok = ();
    type Error = Error;
    type SerializeSeq = SeqBytes<'w>;
    type SerializeTuple = SeqBytes<'w>;
    type SerializeTupleStruct = SeqBytes<'w>;
    type SerializeTupleVariant = VariantBytes<'w>;
    type SerializeMap = MapBytes<'w>;
    type SerializeStruct = StructBytes<'w>;
    type SerializeStructVariant = VariantStructBytes<'w>;

    fn serialize_bool(self, v: bool) -> Result<(), Error> {
        self.writer.add_boolean(v);
        Ok(())
    }
    fn serialize_i8(self, v: i8) -> Result<(), Error> {
        self.writer.add_i8(v);
        Ok(())
    }
    fn serialize_i16(self, v: i16) -> Result<(), Error> {
        self.writer.add_i16(v);
        Ok(())
    }
    fn serialize_i32(self, v: i32) -> Result<(), Error> {
        self.writer.add_i32(v);
        Ok(())
    }
    fn serialize_i64(self, v: i64) -> Result<(), Error> {
        self.writer.add_i64(v);
        Ok(())
    }
    fn serialize_u8(self, v: u8) -> Result<(), Error> {
        self.writer.add_u8(v);
        Ok(())
    }
    fn serialize_u16(self, v: u16) -> Result<(), Error> {
        self.writer.add_u16(v);
        Ok(())
    }
    fn serialize_u32(self, v: u32) -> Result<(), Error> {
        self.writer.add_u32(v);
        Ok(())
    }
    fn serialize_u64(self, v: u64) -> Result<(), Error> {
        self.writer.add_u64(v);
        Ok(())
    }
    fn serialize_f32(self, v: f32) -> Result<(), Error> {
        self.writer.add_f32(v);
        Ok(())
    }
    fn serialize_f64(self, v: f64) -> Result<(), Error> {
        self.writer.add_f64(v);
        Ok(())
    }
    fn serialize_char(self, v: char) -> Result<(), Error> {
        self.writer.add_string(&v.to_string());
        Ok(())
    }
    fn serialize_str(self, v: &str) -> Result<(), Error> {
        self.writer.add_string(v);
        Ok(())
    }
    fn serialize_bytes(self, v: &[u8]) -> Result<(), Error> {
        self.writer.add_array_primitive(TYPE_U8, v.len() as u32);
        self.writer.add_u8_raw_bulk(v);
        Ok(())
    }
    fn serialize_none(self) -> Result<(), Error> {
        self.writer.add_option_none();
        Ok(())
    }
    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<(), Error> {
        self.writer.add_option_some();
        value.serialize(BytesSerializer {
            writer: self.writer,
        })
    }
    fn serialize_unit(self) -> Result<(), Error> {
        self.writer.add_unit();
        Ok(())
    }
    fn serialize_unit_struct(self, _name: &'static str) -> Result<(), Error> {
        self.writer.add_unit();
        Ok(())
    }
    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
    ) -> Result<(), Error> {
        // A bare string names a unit variant — the name-carrying form an
        // unseeded reader (and serde's tagged-enum representations, which
        // route the tag through `deserialize_any`) can recognise; a name-hash
        // cannot be inverted there.
        self.writer.add_string(variant);
        Ok(())
    }
    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        value.serialize(self)
    }
    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        name: &'static str,
        _index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        // One keyvalue entry, keyed by the variant name, carrying the payload.
        let payload =
            value_serde::to_value(value).map_err(|e| ser::Error::custom(e.to_string()))?;
        write_keyvalue(
            self.writer,
            gen_uuid_from_str(name),
            vec![(variant.to_string(), payload)],
        );
        Ok(())
    }
    fn serialize_seq(self, len: Option<usize>) -> Result<SeqBytes<'w>, Error> {
        let len =
            len.ok_or_else(|| ser::Error::custom("the wire format needs sequence lengths"))?;
        self.writer.add_array_primitive(TYPE_VALUE, len as u32);
        Ok(SeqBytes {
            writer: self.writer,
        })
    }
    fn serialize_tuple(self, len: usize) -> Result<SeqBytes<'w>, Error> {
        self.serialize_seq(Some(len))
    }
    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        len: usize,
    ) -> Result<SeqBytes<'w>, Error> {
        self.serialize_seq(Some(len))
    }
    fn serialize_tuple_variant(
        self,
        name: &'static str,
        _index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<VariantBytes<'w>, Error> {
        Ok(VariantBytes {
            writer: self.writer,
            type_name: name,
            variant,
            items: Vec::with_capacity(len),
        })
    }
    fn serialize_map(self, _len: Option<usize>) -> Result<MapBytes<'w>, Error> {
        // Entries buffer as Values so they can sort by key — the deterministic
        // encoding both backends share.
        Ok(MapBytes {
            writer: self.writer,
            key: None,
            fields: Vec::new(),
        })
    }
    fn serialize_struct(self, name: &'static str, len: usize) -> Result<StructBytes<'w>, Error> {
        // An id-less struct travels as a keyvalue under the hash of its type
        // name, its fields carrying their names — not a structure, whose ids
        // come from a declared type.
        Ok(StructBytes {
            writer: self.writer,
            id: gen_uuid_from_str(name),
            fields: Vec::with_capacity(len),
        })
    }
    fn serialize_struct_variant(
        self,
        name: &'static str,
        _index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<VariantStructBytes<'w>, Error> {
        Ok(VariantStructBytes {
            writer: self.writer,
            type_name: name,
            variant,
            fields: Vec::with_capacity(len),
        })
    }
}

struct SeqBytes<'w> {
    writer: &'w mut BufferWriter,
}

impl ser::SerializeSeq for SeqBytes<'_> {
    type Ok = ();
    type Error = Error;
    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        value.serialize(BytesSerializer {
            writer: self.writer,
        })
    }
    fn end(self) -> Result<(), Error> {
        Ok(())
    }
}

impl ser::SerializeTuple for SeqBytes<'_> {
    type Ok = ();
    type Error = Error;
    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        ser::SerializeSeq::serialize_element(self, value)
    }
    fn end(self) -> Result<(), Error> {
        Ok(())
    }
}

impl ser::SerializeTupleStruct for SeqBytes<'_> {
    type Ok = ();
    type Error = Error;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        ser::SerializeSeq::serialize_element(self, value)
    }
    fn end(self) -> Result<(), Error> {
        Ok(())
    }
}

impl ser::SerializeTupleVariant for SeqBytes<'_> {
    type Ok = ();
    type Error = Error;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        ser::SerializeSeq::serialize_element(self, value)
    }
    fn end(self) -> Result<(), Error> {
        Ok(())
    }
}

/// Write `fields` as a keyvalue under `id`: a name-keyed map on the wire, its
/// entries sorted by key so the encoding is deterministic (matching the `Value`
/// backend). Each field carries its name and a name-hash field id — the name is
/// the authoritative key.
fn write_keyvalue(
    writer: &mut BufferWriter,
    id: uuid::Uuid,
    mut fields: Vec<(String, arora_types::value::Value)>,
) {
    fields.sort_by(|a, b| a.0.cmp(&b.0));
    writer.begin_map(id.as_bytes(), fields.len() as u32);
    for (key, value) in &fields {
        writer.add_map_field_key(key);
        writer.add_uuid_raw(gen_uuid_from_str(key).as_bytes());
        serialize_to_writer(value, writer);
    }
}

struct MapBytes<'w> {
    writer: &'w mut BufferWriter,
    key: Option<String>,
    fields: Vec<(String, arora_types::value::Value)>,
}

impl ser::SerializeMap for MapBytes<'_> {
    type Ok = ();
    type Error = Error;
    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> Result<(), Error> {
        match value_serde::to_value(key).map_err(|e| ser::Error::custom(e.to_string()))? {
            arora_types::value::Value::String(name) => {
                self.key = Some(name);
                Ok(())
            }
            other => Err(ser::Error::custom(format!(
                "map keys must be strings to become a KeyValue, got {other:?}"
            ))),
        }
    }
    fn serialize_value<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        let key = self
            .key
            .take()
            .ok_or_else(|| ser::Error::custom("map value without a key"))?;
        let value = value_serde::to_value(value).map_err(|e| ser::Error::custom(e.to_string()))?;
        self.fields.push((key, value));
        Ok(())
    }
    fn end(self) -> Result<(), Error> {
        write_keyvalue(self.writer, value_serde::map_id(), self.fields);
        Ok(())
    }
}

struct StructBytes<'w> {
    writer: &'w mut BufferWriter,
    id: uuid::Uuid,
    fields: Vec<(String, arora_types::value::Value)>,
}

impl ser::SerializeStruct for StructBytes<'_> {
    type Ok = ();
    type Error = Error;
    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        // Fields buffer as Values so they can sort by key at the end — the
        // deterministic keyvalue encoding both backends share.
        let value = value_serde::to_value(value).map_err(|e| ser::Error::custom(e.to_string()))?;
        self.fields.push((key.to_string(), value));
        Ok(())
    }
    fn end(self) -> Result<(), Error> {
        write_keyvalue(self.writer, self.id, self.fields);
        Ok(())
    }
}

/// A tuple variant's items, buffered as `Value`s: the payload travels as one
/// value array inside the variant-named keyvalue entry.
struct VariantBytes<'w> {
    writer: &'w mut BufferWriter,
    type_name: &'static str,
    variant: &'static str,
    items: Vec<arora_types::value::Value>,
}

impl ser::SerializeTupleVariant for VariantBytes<'_> {
    type Ok = ();
    type Error = Error;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        self.items
            .push(value_serde::to_value(value).map_err(|e| ser::Error::custom(e.to_string()))?);
        Ok(())
    }
    fn end(self) -> Result<(), Error> {
        write_keyvalue(
            self.writer,
            gen_uuid_from_str(self.type_name),
            vec![(
                self.variant.to_string(),
                arora_types::value::Value::ArrayValue(self.items),
            )],
        );
        Ok(())
    }
}

/// A struct variant's fields, buffered as `Value`s: the payload travels as an
/// inner keyvalue inside the variant-named entry.
struct VariantStructBytes<'w> {
    writer: &'w mut BufferWriter,
    type_name: &'static str,
    variant: &'static str,
    fields: Vec<(String, arora_types::value::Value)>,
}

impl ser::SerializeStructVariant for VariantStructBytes<'_> {
    type Ok = ();
    type Error = Error;
    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        self.fields.push((
            key.to_string(),
            value_serde::to_value(value).map_err(|e| ser::Error::custom(e.to_string()))?,
        ));
        Ok(())
    }
    fn end(self) -> Result<(), Error> {
        let mut inner =
            arora_types::keyvalue::KeyValue::new_with_id(gen_uuid_from_str(self.variant));
        for (name, value) in self.fields {
            let id = gen_uuid_from_str(&name);
            inner.set_field(
                arora_types::keyvalue::KeyValueField::new_with_id_and_option(name, id, Some(value)),
            );
        }
        write_keyvalue(
            self.writer,
            gen_uuid_from_str(self.type_name),
            vec![(
                self.variant.to_string(),
                arora_types::value::Value::KeyValue(inner),
            )],
        );
        Ok(())
    }
}

// ---- deserialization -------------------------------------------------------

struct BytesDeserializer<'de, 'r> {
    reader: &'r mut BufferReader<'de>,
}

impl<'de> de::Deserializer<'de> for BytesDeserializer<'de, '_> {
    type Error = Error;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        match self.reader.next_type() {
            Some(TYPE_UNIT) => visitor.visit_unit(),
            Some(TYPE_BOOLEAN) => visitor.visit_bool(self.reader.get_boolean()),
            Some(TYPE_U8) => visitor.visit_u8(self.reader.get_u8()),
            Some(TYPE_U16) => visitor.visit_u16(self.reader.get_u16()),
            Some(TYPE_U32) => visitor.visit_u32(self.reader.get_u32()),
            Some(TYPE_U64) => visitor.visit_u64(self.reader.get_u64()),
            Some(TYPE_I8) => visitor.visit_i8(self.reader.get_i8()),
            Some(TYPE_I16) => visitor.visit_i16(self.reader.get_i16()),
            Some(TYPE_I32) => visitor.visit_i32(self.reader.get_i32()),
            Some(TYPE_I64) => visitor.visit_i64(self.reader.get_i64()),
            Some(TYPE_F32) => visitor.visit_f32(self.reader.get_f32()),
            Some(TYPE_F64) => visitor.visit_f64(self.reader.get_f64()),
            Some(TYPE_STRING) => visitor.visit_borrowed_str(self.reader.get_string()),
            Some(TYPE_OPTION) => {
                if self.reader.get_option_presence() {
                    visitor.visit_some(BytesDeserializer {
                        reader: self.reader,
                    })
                } else {
                    visitor.visit_none()
                }
            }
            Some(TYPE_UUID) => {
                let bytes = self.reader.get_uuid();
                let uuid = uuid::Uuid::from_slice(bytes)
                    .map_err(|e| de::Error::custom(format!("malformed uuid: {e}")))?;
                visitor.visit_string(uuid.to_string())
            }
            Some(TYPE_ARRAY) => {
                let (ty, count) = self.reader.get_array();
                if ty == TYPE_VALUE {
                    visitor.visit_seq(TaggedSeq {
                        reader: self.reader,
                        remaining: count as usize,
                    })
                } else if ty == TYPE_STRING {
                    visitor.visit_seq(StringSeq {
                        reader: self.reader,
                        remaining: count as usize,
                    })
                } else {
                    // Typed primitive arrays store raw elements after one
                    // alignment, no per-element tags.
                    self.reader.align();
                    visitor.visit_seq(RawSeq {
                        reader: self.reader,
                        ty,
                        remaining: count as usize,
                    })
                }
            }
            Some(TYPE_MAP) => {
                let (_id, count) = self.reader.get_map();
                visitor.visit_map(MapBytesAccess {
                    reader: self.reader,
                    remaining: count as usize,
                    pending_value: false,
                })
            }
            Some(TYPE_STRUCTURE) => Err(de::Error::custom(
                "a structure needs its declared field names; use a struct target",
            )),
            Some(TYPE_ENUMERATION) => Err(de::Error::custom(
                "an enumeration needs its declared variant names; use an enum target",
            )),
            Some(kind) => Err(de::Error::custom(format!("invalid type tag {kind}"))),
            None => Err(de::Error::custom("unexpected end of buffer")),
        }
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        match self.reader.next_type() {
            Some(TYPE_OPTION) => {
                if self.reader.get_option_presence() {
                    visitor.visit_some(BytesDeserializer {
                        reader: self.reader,
                    })
                } else {
                    visitor.visit_none()
                }
            }
            Some(TYPE_UNIT) => visitor.visit_none(),
            Some(other) => Err(de::Error::custom(format!(
                "expected an option, got type tag {other}"
            ))),
            None => Err(de::Error::custom("unexpected end of buffer")),
        }
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        match self.reader.next_type() {
            Some(TYPE_STRUCTURE) => {
                let (_id, count) = self.reader.get_structure();
                visitor.visit_map(StructBytesAccess {
                    reader: self.reader,
                    fields,
                    remaining: count as usize,
                    pending: None,
                })
            }
            Some(TYPE_MAP) => {
                let (_id, count) = self.reader.get_map();
                visitor.visit_map(MapBytesAccess {
                    reader: self.reader,
                    remaining: count as usize,
                    pending_value: false,
                })
            }
            Some(other) => Err(de::Error::custom(format!(
                "expected a structure, got type tag {other}"
            ))),
            None => Err(de::Error::custom("unexpected end of buffer")),
        }
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _name: &'static str,
        variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        match self.reader.next_type() {
            // The name-carrying forms: a bare string names a unit variant; a
            // single-entry keyvalue keys the payload by the variant name.
            Some(TYPE_STRING) => {
                let name = self.reader.get_string();
                let variant = variants
                    .iter()
                    .find(|candidate| **candidate == name)
                    .ok_or_else(|| {
                        de::Error::custom(format!("variant `{name}` matches none of {variants:?}"))
                    })?;
                visitor.visit_enum(de::value::StrDeserializer::new(variant))
            }
            Some(TYPE_MAP) => {
                let (_id, count) = self.reader.get_map();
                if count != 1 {
                    return Err(de::Error::custom(format!(
                        "an enum keyvalue carries exactly one variant entry, got {count}"
                    )));
                }
                let name = self.reader.get_map_field_key();
                let _field_id = self.reader.get_uuid();
                let variant = variants
                    .iter()
                    .find(|candidate| **candidate == name)
                    .ok_or_else(|| {
                        de::Error::custom(format!("variant `{name}` matches none of {variants:?}"))
                    })?;
                visitor.visit_enum(EnumBytesAccess {
                    variant,
                    reader: self.reader,
                })
            }
            // Bytes written before the name-carrying form: the variant tag as
            // a wire enumeration whose ids hash the names.
            Some(TYPE_ENUMERATION) => {
                let _type_id = self.reader.get_structure_field();
                let variant_id = self.reader.get_enumeration_value_raw();
                let variant_id = uuid::Uuid::from_slice(variant_id)
                    .map_err(|e| de::Error::custom(format!("malformed variant id: {e}")))?;
                let variant = variants
                    .iter()
                    .find(|candidate| gen_uuid_from_str(candidate) == variant_id)
                    .ok_or_else(|| {
                        de::Error::custom(format!(
                            "variant id {variant_id} matches none of {variants:?}"
                        ))
                    })?;
                visitor.visit_enum(EnumBytesAccess {
                    variant,
                    reader: self.reader,
                })
            }
            Some(other) => Err(de::Error::custom(format!(
                "expected an enumeration, got type tag {other}"
            ))),
            None => Err(de::Error::custom("unexpected end of buffer")),
        }
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Error> {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_ignored_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        // Consume one self-describing value, whatever it is.
        let _ = deserialize_from_reader(self.reader);
        visitor.visit_unit()
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf unit unit_struct seq tuple tuple_struct map identifier
    }
}

struct TaggedSeq<'de, 'r> {
    reader: &'r mut BufferReader<'de>,
    remaining: usize,
}

impl<'de> SeqAccess<'de> for TaggedSeq<'de, '_> {
    type Error = Error;
    fn next_element_seed<T: DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, Error> {
        if self.remaining == 0 {
            return Ok(None);
        }
        self.remaining -= 1;
        seed.deserialize(BytesDeserializer {
            reader: self.reader,
        })
        .map(Some)
    }
    fn size_hint(&self) -> Option<usize> {
        Some(self.remaining)
    }
}

struct StringSeq<'de, 'r> {
    reader: &'r mut BufferReader<'de>,
    remaining: usize,
}

impl<'de> SeqAccess<'de> for StringSeq<'de, '_> {
    type Error = Error;
    fn next_element_seed<T: DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, Error> {
        if self.remaining == 0 {
            return Ok(None);
        }
        self.remaining -= 1;
        let text = self.reader.get_string();
        seed.deserialize(de::value::BorrowedStrDeserializer::new(text))
            .map(Some)
    }
    fn size_hint(&self) -> Option<usize> {
        Some(self.remaining)
    }
}

struct RawSeq<'de, 'r> {
    reader: &'r mut BufferReader<'de>,
    ty: u8,
    remaining: usize,
}

impl<'de> SeqAccess<'de> for RawSeq<'de, '_> {
    type Error = Error;
    fn next_element_seed<T: DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, Error> {
        if self.remaining == 0 {
            return Ok(None);
        }
        self.remaining -= 1;
        seed.deserialize(RawPrimitive {
            reader: self.reader,
            ty: self.ty,
        })
        .map(Some)
    }
    fn size_hint(&self) -> Option<usize> {
        Some(self.remaining)
    }
}

/// One untagged element of a typed primitive array.
struct RawPrimitive<'de, 'r> {
    reader: &'r mut BufferReader<'de>,
    ty: u8,
}

impl<'de> de::Deserializer<'de> for RawPrimitive<'de, '_> {
    type Error = Error;
    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        match self.ty {
            TYPE_BOOLEAN => visitor.visit_bool(self.reader.get_boolean()),
            TYPE_U8 => visitor.visit_u8(self.reader.get_u8()),
            TYPE_U16 => visitor.visit_u16(self.reader.get_u16()),
            TYPE_U32 => visitor.visit_u32(self.reader.get_u32()),
            TYPE_U64 => visitor.visit_u64(self.reader.get_u64()),
            TYPE_I8 => visitor.visit_i8(self.reader.get_i8()),
            TYPE_I16 => visitor.visit_i16(self.reader.get_i16()),
            TYPE_I32 => visitor.visit_i32(self.reader.get_i32()),
            TYPE_I64 => visitor.visit_i64(self.reader.get_i64()),
            TYPE_F32 => visitor.visit_f32(self.reader.get_f32()),
            TYPE_F64 => visitor.visit_f64(self.reader.get_f64()),
            other => Err(de::Error::custom(format!(
                "unsupported raw array element type {other}"
            ))),
        }
    }
    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf option unit unit_struct newtype_struct seq tuple
        tuple_struct map struct enum identifier ignored_any
    }
}

struct MapBytesAccess<'de, 'r> {
    reader: &'r mut BufferReader<'de>,
    remaining: usize,
    pending_value: bool,
}

impl<'de> MapAccess<'de> for MapBytesAccess<'de, '_> {
    type Error = Error;
    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, Error> {
        if self.remaining == 0 {
            return Ok(None);
        }
        self.remaining -= 1;
        let key = self.reader.get_map_field_key();
        let _field_id = self.reader.get_uuid();
        self.pending_value = true;
        seed.deserialize(de::value::BorrowedStrDeserializer::new(key))
            .map(Some)
    }
    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, Error> {
        if !self.pending_value {
            return Err(de::Error::custom("map value without a key"));
        }
        self.pending_value = false;
        seed.deserialize(BytesDeserializer {
            reader: self.reader,
        })
    }
    fn size_hint(&self) -> Option<usize> {
        Some(self.remaining)
    }
}

struct StructBytesAccess<'de, 'r> {
    reader: &'r mut BufferReader<'de>,
    fields: &'static [&'static str],
    remaining: usize,
    pending: Option<&'static str>,
}

impl<'de> MapAccess<'de> for StructBytesAccess<'de, '_> {
    type Error = Error;
    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, Error> {
        // Resolve each stored field id back to its declared name; fields with
        // ids outside the declaration are consumed and skipped.
        while self.remaining > 0 {
            self.remaining -= 1;
            let field_id = self.reader.get_structure_field();
            let field_id = uuid::Uuid::from_slice(field_id)
                .map_err(|e| de::Error::custom(format!("malformed field id: {e}")))?;
            match self
                .fields
                .iter()
                .find(|candidate| gen_uuid_from_str(candidate) == field_id)
            {
                Some(name) => {
                    self.pending = Some(name);
                    return seed.deserialize(name.into_deserializer()).map(Some);
                }
                None => {
                    let _ = deserialize_from_reader(self.reader);
                }
            }
        }
        Ok(None)
    }
    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, Error> {
        self.pending
            .take()
            .ok_or_else(|| de::Error::custom("struct value without a field"))?;
        seed.deserialize(BytesDeserializer {
            reader: self.reader,
        })
    }
}

struct EnumBytesAccess<'de, 'r> {
    variant: &'static str,
    reader: &'r mut BufferReader<'de>,
}

impl<'de, 'r> EnumAccess<'de> for EnumBytesAccess<'de, 'r> {
    type Error = Error;
    type Variant = VariantBytesAccess<'de, 'r>;
    fn variant_seed<V: DeserializeSeed<'de>>(
        self,
        seed: V,
    ) -> Result<(V::Value, Self::Variant), Error> {
        let variant = seed.deserialize(self.variant.into_deserializer())?;
        Ok((
            variant,
            VariantBytesAccess {
                reader: self.reader,
            },
        ))
    }
}

struct VariantBytesAccess<'de, 'r> {
    reader: &'r mut BufferReader<'de>,
}

impl<'de> VariantAccess<'de> for VariantBytesAccess<'de, '_> {
    type Error = Error;
    fn unit_variant(self) -> Result<(), Error> {
        match self.reader.next_type() {
            Some(TYPE_UNIT) => Ok(()),
            Some(other) => Err(de::Error::custom(format!(
                "expected a unit variant payload, got type tag {other}"
            ))),
            None => Err(de::Error::custom("unexpected end of buffer")),
        }
    }
    fn newtype_variant_seed<T: DeserializeSeed<'de>>(self, seed: T) -> Result<T::Value, Error> {
        seed.deserialize(BytesDeserializer {
            reader: self.reader,
        })
    }
    fn tuple_variant<V: Visitor<'de>>(self, _len: usize, visitor: V) -> Result<V::Value, Error> {
        match self.reader.next_type() {
            Some(TYPE_ARRAY) => {
                let (ty, count) = self.reader.get_array();
                if ty != TYPE_VALUE {
                    return Err(de::Error::custom(
                        "expected a value array as the tuple variant payload",
                    ));
                }
                visitor.visit_seq(TaggedSeq {
                    reader: self.reader,
                    remaining: count as usize,
                })
            }
            _ => Err(de::Error::custom("expected a tuple variant payload")),
        }
    }
    fn struct_variant<V: Visitor<'de>>(
        self,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        match self.reader.next_type() {
            // The variant's payload is an id-less struct: a keyvalue matched by
            // name. A hand-written structure whose field ids are name hashes is
            // still accepted.
            Some(TYPE_MAP) => {
                let (_id, count) = self.reader.get_map();
                visitor.visit_map(MapBytesAccess {
                    reader: self.reader,
                    remaining: count as usize,
                    pending_value: false,
                })
            }
            Some(TYPE_STRUCTURE) => {
                let (_id, count) = self.reader.get_structure();
                visitor.visit_map(StructBytesAccess {
                    reader: self.reader,
                    fields,
                    remaining: count as usize,
                    pending: None,
                })
            }
            _ => Err(de::Error::custom("expected a struct variant payload")),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::froto_value;
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    enum Shape {
        Empty,
        Circle(f32),
        Segment(f32, f32),
        Box { width: f32, height: f32 },
    }

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct Sample {
        name: String,
        weight: f64,
        tags: Vec<String>,
        lucky: Option<u32>,
        unlucky: Option<u32>,
        shape: Shape,
        pairs: HashMap<String, i32>,
        ratio: (u8, u8),
    }

    fn sample() -> Sample {
        Sample {
            name: "arora".to_string(),
            weight: 0.5,
            tags: vec!["a".to_string(), "b".to_string()],
            lucky: Some(7),
            unlucky: None,
            shape: Shape::Box {
                width: 2.0,
                height: 3.0,
            },
            pairs: HashMap::from([("x".to_string(), 1), ("y".to_string(), -1)]),
            ratio: (3, 4),
        }
    }

    #[test]
    fn round_trips_without_a_value() {
        let bytes = to_bytes(&sample()).unwrap();
        let back: Sample = from_bytes(&bytes).unwrap();
        assert_eq!(back, sample());
    }

    #[test]
    fn a_struct_writes_a_keyvalue() {
        // An id-less struct carries its field names, so it travels as a
        // keyvalue — a name-hash type id would be a false identity the moment a
        // field or the struct is renamed. Inspecting the bytes as a `Value`
        // confirms the wire form.
        let bytes = to_bytes(&sample()).unwrap();
        assert!(matches!(
            froto_value::deserialize(&bytes),
            arora_types::value::Value::KeyValue(_)
        ));
    }

    #[test]
    fn both_backends_write_identical_bytes() {
        // The direct path and the Value path are the same wire format: a
        // payload written by one side reads on the other.
        let via_value =
            froto_value::serialize(&arora_types::value_serde::to_value(&sample()).unwrap());
        let direct = to_bytes(&sample()).unwrap();
        assert_eq!(via_value, direct);

        let back: Sample = from_bytes(&via_value).unwrap();
        assert_eq!(back, sample());
    }

    #[test]
    fn reads_values_written_by_hand() {
        // A typed f32 array (raw bulk elements) decodes as a plain sequence.
        let bytes = froto_value::serialize(&arora_types::value::Value::ArrayF32(vec![1.0, 2.0]));
        let back: Vec<f32> = from_bytes(&bytes).unwrap();
        assert_eq!(back, vec![1.0, 2.0]);
    }

    #[test]
    fn every_enum_shape_round_trips() {
        for shape in [
            Shape::Empty,
            Shape::Circle(1.5),
            Shape::Segment(0.0, 2.0),
            Shape::Box {
                width: 1.0,
                height: 2.0,
            },
        ] {
            let bytes = to_bytes(&shape).unwrap();
            let back: Shape = from_bytes(&bytes).unwrap();
            assert_eq!(back, shape);
        }
    }
}
