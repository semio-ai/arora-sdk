//! Runtime-type-directed (de)serialization of a [`Value`] against a
//! [`low::Type`], for wire formats whose fields are keyed by runtime UUID.
//!
//! A [`ValueWriter`]/[`ValueReader`] is a *format* — arora-buffers, or ROS 2
//! CDR — expressed as primitive read/write ops plus struct framing. These are
//! the helpers you implement to teach arora a new wire format; the walk below
//! drives them.
//!
//! [`write_value`]/[`read_value`] are the *type-directed walk*: a single runtime
//! recursion over a [`low::Type`] and a [`TypeRegistry`] (which resolves the
//! [`TypeRef`]s that name nested types), rather than monomorphised-per-type
//! code. It is the counterpart of what serde's `Serialize`/`Deserialize` do per
//! type, expressed once over arora's runtime `Value`/`Type` vocabulary. (serde
//! *can* be seeded with a runtime schema — see [`super::bridge`], which drives
//! the Rust-type ⇄ `Value` direction — but a direct `Value` walk is simpler for
//! the dynamic case where no Rust type is in hand.)
//!
//! Struct fields are walked in the type's **declared order** — [`low::Structure`]
//! stores them in an insertion-ordered `IndexMap`, which is load-bearing:
//! generated module readers consume fields positionally in that order, so this
//! walk must match them.
//!
//! Supported: primitive scalars (incl. string), nested structures, and
//! homogeneous arrays — a [`TypeRef::Array`] of a scalar element maps to a typed
//! [`Value::ArrayU8`]/`…`, of a structure element to a [`Value::ArrayStructure`].
//! Enumerations, options and maps error explicitly rather than mis-encode; they
//! extend the trait and walk the same way.

use uuid::Uuid;

use super::{Error, Result};
use crate::module::low::TypeRef;
use crate::ty::{self, low, TypeRegistry};
use crate::value::{Structure, StructureField, StructureWithoutId, Value};

fn err<T>(message: impl Into<String>) -> Result<T> {
  Err(Error::new(message))
}

/// A format a [`Value`] is written to, one datum at a time. Struct framing is
/// explicit so a non-self-describing format (CDR) can emit length/alignment and
/// a self-describing one (arora-buffers) can emit type tags.
pub trait ValueWriter {
  fn write_unit(&mut self) -> Result<()>;
  fn write_bool(&mut self, v: bool) -> Result<()>;
  fn write_u8(&mut self, v: u8) -> Result<()>;
  fn write_u16(&mut self, v: u16) -> Result<()>;
  fn write_u32(&mut self, v: u32) -> Result<()>;
  fn write_u64(&mut self, v: u64) -> Result<()>;
  fn write_i8(&mut self, v: i8) -> Result<()>;
  fn write_i16(&mut self, v: i16) -> Result<()>;
  fn write_i32(&mut self, v: i32) -> Result<()>;
  fn write_i64(&mut self, v: i64) -> Result<()>;
  fn write_f32(&mut self, v: f32) -> Result<()>;
  fn write_f64(&mut self, v: f64) -> Result<()>;
  fn write_string(&mut self, v: &str) -> Result<()>;

  /// Begin a structure of `field_count` fields.
  fn begin_struct(&mut self, id: Uuid, field_count: usize) -> Result<()>;
  /// Announce the next field's id; its value follows via the datum ops.
  fn begin_field(&mut self, id: Uuid) -> Result<()>;

  // Homogeneous scalar arrays are written whole, not element-by-element: a
  // self-describing format (arora-buffers) tags the element type *once* at the
  // array head and then writes the elements raw, so a per-element datum op —
  // which would re-tag every element — is the wrong shape. Each backend frames
  // its own way (buffers: tag + raw bulk; CDR: 4-aligned count + elements), so
  // these are honest bulk operations rather than sugar over the scalar ops.
  fn write_bool_array(&mut self, v: &[bool]) -> Result<()>;
  fn write_u8_array(&mut self, v: &[u8]) -> Result<()>;
  fn write_u16_array(&mut self, v: &[u16]) -> Result<()>;
  fn write_u32_array(&mut self, v: &[u32]) -> Result<()>;
  fn write_u64_array(&mut self, v: &[u64]) -> Result<()>;
  fn write_i8_array(&mut self, v: &[i8]) -> Result<()>;
  fn write_i16_array(&mut self, v: &[i16]) -> Result<()>;
  fn write_i32_array(&mut self, v: &[i32]) -> Result<()>;
  fn write_i64_array(&mut self, v: &[i64]) -> Result<()>;
  fn write_f32_array(&mut self, v: &[f32]) -> Result<()>;
  fn write_f64_array(&mut self, v: &[f64]) -> Result<()>;
  fn write_string_array(&mut self, v: &[String]) -> Result<()>;

  /// Begin an array of `len` structures, each of type `element_id`. The element
  /// type is named once here; the `len` element bodies follow, each introduced
  /// by [`begin_struct_element`](Self::begin_struct_element) — they carry no id
  /// of their own. A positional format (CDR) emits the count and alignment; a
  /// self-describing one emits its array tag and the element type id.
  fn begin_struct_array(&mut self, element_id: Uuid, len: usize) -> Result<()>;
  /// Begin one element of a [`begin_struct_array`](Self::begin_struct_array),
  /// carrying `field_count` fields (which follow via the field/datum ops). The
  /// element's type is already fixed by the array head, so no id is written: a
  /// self-describing format emits only the field count, a positional one nothing.
  fn begin_struct_element(&mut self, field_count: usize) -> Result<()>;
}

/// A format a [`Value`] is read from, type-directed by the walk. A
/// self-describing format validates its inline tag against the request.
pub trait ValueReader {
  fn read_unit(&mut self) -> Result<()>;
  fn read_bool(&mut self) -> Result<bool>;
  fn read_u8(&mut self) -> Result<u8>;
  fn read_u16(&mut self) -> Result<u16>;
  fn read_u32(&mut self) -> Result<u32>;
  fn read_u64(&mut self) -> Result<u64>;
  fn read_i8(&mut self) -> Result<i8>;
  fn read_i16(&mut self) -> Result<i16>;
  fn read_i32(&mut self) -> Result<i32>;
  fn read_i64(&mut self) -> Result<i64>;
  fn read_f32(&mut self) -> Result<f32>;
  fn read_f64(&mut self) -> Result<f64>;
  fn read_string(&mut self) -> Result<String>;

  /// Enter a structure the walk expects to carry `expected_id` and
  /// `field_count` fields. A self-describing format reads its inline header and
  /// validates it against these; a positional format (CDR) takes the shape from
  /// the type and reads nothing here.
  fn enter_struct(&mut self, expected_id: Uuid, field_count: usize) -> Result<()>;
  /// Enter the next field, which the walk expects to carry `expected_id`. A
  /// self-describing format reads and validates its inline field id; a
  /// positional format reads nothing.
  fn enter_field(&mut self, expected_id: Uuid) -> Result<()>;

  // Counterparts of the `write_*_array` bulk writers: read a whole homogeneous
  // scalar array. A self-describing format validates the array head's element
  // tag against the type the walk asks for.
  fn read_bool_array(&mut self) -> Result<Vec<bool>>;
  fn read_u8_array(&mut self) -> Result<Vec<u8>>;
  fn read_u16_array(&mut self) -> Result<Vec<u16>>;
  fn read_u32_array(&mut self) -> Result<Vec<u32>>;
  fn read_u64_array(&mut self) -> Result<Vec<u64>>;
  fn read_i8_array(&mut self) -> Result<Vec<i8>>;
  fn read_i16_array(&mut self) -> Result<Vec<i16>>;
  fn read_i32_array(&mut self) -> Result<Vec<i32>>;
  fn read_i64_array(&mut self) -> Result<Vec<i64>>;
  fn read_f32_array(&mut self) -> Result<Vec<f32>>;
  fn read_f64_array(&mut self) -> Result<Vec<f64>>;
  fn read_string_array(&mut self) -> Result<Vec<String>>;

  /// Enter an array of structures of type `element_id`, returning the element
  /// count. Each of the `count` element bodies is then read via
  /// [`enter_struct_element`](Self::enter_struct_element) then the field ops. A
  /// positional format (CDR) reads the count here; a self-describing one reads
  /// and validates its array header, including the element type id.
  fn enter_struct_array(&mut self, element_id: Uuid) -> Result<usize>;
  /// Enter one element of an [`enter_struct_array`](Self::enter_struct_array),
  /// which the walk expects to carry `field_count` fields. A self-describing
  /// format reads and validates the on-wire field count; a positional one takes
  /// it from the type and reads nothing.
  fn enter_struct_element(&mut self, field_count: usize) -> Result<()>;
}

/// Serialize `value` against `ty` into `writer`. Errors if `value` does not
/// match `ty` — so a producer cannot emit a buffer a remote expecting `ty`
/// could not read.
pub fn write_value<W: ValueWriter>(
  ty: &low::Type,
  registry: &TypeRegistry,
  value: &Value,
  writer: &mut W,
) -> Result<()> {
  match &ty.kind {
    low::TypeKind::Primitive(type_ref) => write_by_ref(type_ref, registry, value, writer),
    low::TypeKind::Structure(structure) => {
      write_structure(ty.id, structure, registry, value, writer)
    }
    low::TypeKind::Enumeration(_) => err("enumeration types are not supported yet"),
  }
}

/// Deserialize a [`Value`] from `reader` against `ty`.
pub fn read_value<R: ValueReader>(
  ty: &low::Type,
  registry: &TypeRegistry,
  reader: &mut R,
) -> Result<Value> {
  match &ty.kind {
    low::TypeKind::Primitive(type_ref) => read_by_ref(type_ref, registry, reader),
    low::TypeKind::Structure(structure) => read_structure(ty.id, structure, registry, reader),
    low::TypeKind::Enumeration(_) => err("enumeration types are not supported yet"),
  }
}

fn write_structure<W: ValueWriter>(
  id: Uuid,
  structure: &low::Structure,
  registry: &TypeRegistry,
  value: &Value,
  writer: &mut W,
) -> Result<()> {
  let actual = match value {
    Value::Structure(s) => s,
    other => return err(format!("expected a structure value, got {other}")),
  };
  if actual.id != id {
    return err(format!(
      "structure id {} does not match type id {id}",
      actual.id
    ));
  }
  write_structure_fields(id, structure, registry, &actual.fields, writer)
}

/// Write a standalone structure's `fields` against `structure` in declared
/// order, with the id-bearing struct header.
fn write_structure_fields<W: ValueWriter>(
  id: Uuid,
  structure: &low::Structure,
  registry: &TypeRegistry,
  fields: &[StructureField],
  writer: &mut W,
) -> Result<()> {
  check_field_count(structure, fields)?;
  writer.begin_struct(id, structure.fields.len())?;
  write_fields(structure, registry, fields, writer)
}

fn check_field_count(structure: &low::Structure, fields: &[StructureField]) -> Result<()> {
  if fields.len() != structure.fields.len() {
    return err(format!(
      "structure has {} fields, type declares {}",
      fields.len(),
      structure.fields.len()
    ));
  }
  Ok(())
}

/// The field body shared by standalone structures and array-of-structure
/// elements — the caller writes the header (id-bearing or element-only) first.
/// Declared order (IndexMap) drives the wire order; the value's fields must be
/// in that same order, field id by field id.
fn write_fields<W: ValueWriter>(
  structure: &low::Structure,
  registry: &TypeRegistry,
  fields: &[StructureField],
  writer: &mut W,
) -> Result<()> {
  for ((field_id, field), actual_field) in structure.fields.iter().zip(fields) {
    if actual_field.id != *field_id {
      return err(format!(
        "field id {} does not match the type's declared field {field_id}",
        actual_field.id
      ));
    }
    writer.begin_field(*field_id)?;
    write_by_ref(&field.type_ref, registry, &actual_field.value, writer)?;
  }
  Ok(())
}

fn read_structure<R: ValueReader>(
  id: Uuid,
  structure: &low::Structure,
  registry: &TypeRegistry,
  reader: &mut R,
) -> Result<Value> {
  Ok(Value::Structure(Structure {
    id,
    fields: read_structure_fields(id, structure, registry, reader)?,
  }))
}

/// Read a standalone structure's fields against `structure` in declared order,
/// consuming the id-bearing struct header first.
fn read_structure_fields<R: ValueReader>(
  id: Uuid,
  structure: &low::Structure,
  registry: &TypeRegistry,
  reader: &mut R,
) -> Result<Vec<StructureField>> {
  reader.enter_struct(id, structure.fields.len())?;
  read_fields(structure, registry, reader)
}

/// The field body shared by standalone structures and array-of-structure
/// elements — the caller consumes the header first. The type drives the shape;
/// the reader validates (self-describing) or takes it as given (positional).
/// Field ids come from the type, not the wire.
fn read_fields<R: ValueReader>(
  structure: &low::Structure,
  registry: &TypeRegistry,
  reader: &mut R,
) -> Result<Vec<StructureField>> {
  let mut fields = Vec::with_capacity(structure.fields.len());
  for (field_id, field) in &structure.fields {
    reader.enter_field(*field_id)?;
    fields.push(StructureField {
      id: *field_id,
      value: Box::new(read_by_ref(&field.type_ref, registry, reader)?),
    });
  }
  Ok(fields)
}

/// Write a value whose type is named by `type_ref`: a well-known primitive is
/// written directly; any other id is resolved in `registry` and recursed.
fn write_by_ref<W: ValueWriter>(
  type_ref: &TypeRef,
  registry: &TypeRegistry,
  value: &Value,
  writer: &mut W,
) -> Result<()> {
  match type_ref {
    TypeRef::Scalar { id } => {
      if ty::PRIMITIVE_IDS.contains(id) {
        write_scalar(*id, value, writer)
      } else {
        let nested = registry
          .get(id)
          .ok_or_else(|| Error::new(format!("type {id} not found in the registry")))?;
        write_value(nested, registry, value, writer)
      }
    }
    TypeRef::Array { id } => write_array(*id, registry, value, writer),
    TypeRef::Map { .. } => err("map types are not supported yet"),
  }
}

fn read_by_ref<R: ValueReader>(
  type_ref: &TypeRef,
  registry: &TypeRegistry,
  reader: &mut R,
) -> Result<Value> {
  match type_ref {
    TypeRef::Scalar { id } => {
      if ty::PRIMITIVE_IDS.contains(id) {
        read_scalar(*id, reader)
      } else {
        let nested = registry
          .get(id)
          .ok_or_else(|| Error::new(format!("type {id} not found in the registry")))?;
        read_value(nested, registry, reader)
      }
    }
    TypeRef::Array { id } => read_array(*id, registry, reader),
    TypeRef::Map { .. } => err("map types are not supported yet"),
  }
}

/// Write an array whose element type is `element_id`: a registered id is a
/// nested structure element (`Value::ArrayStructure`), otherwise a well-known
/// scalar element (`Value::ArrayU8`/`…`).
fn write_array<W: ValueWriter>(
  element_id: Uuid,
  registry: &TypeRegistry,
  value: &Value,
  writer: &mut W,
) -> Result<()> {
  match registry.get(&element_id) {
    Some(element_ty) => {
      let low::TypeKind::Structure(structure) = &element_ty.kind else {
        return err(format!(
          "array element type {element_id} is not a structure"
        ));
      };
      let (id, elements) = match value {
        Value::ArrayStructure { id, elements } => (*id, elements),
        other => return err(format!("expected an array of structures, got {other}")),
      };
      if id != element_id {
        return err(format!(
          "array-of-structure element id {id} does not match type element id {element_id}"
        ));
      }
      // Framing: the element type id is named once, in the array head; each
      // element is a headerless struct body (field count + fields), never
      // re-tagged. This matches the self-describing `Value` backend
      // (arora-buffers `serde_uuid`), so the two encode a struct array
      // identically.
      writer.begin_struct_array(element_id, elements.len())?;
      for element in elements {
        check_field_count(structure, &element.fields)?;
        writer.begin_struct_element(structure.fields.len())?;
        write_fields(structure, registry, &element.fields, writer)?;
      }
      Ok(())
    }
    None => write_scalar_array(element_id, value, writer),
  }
}

/// Write a scalar array (`Value::Array*`) whose elements are of the well-known
/// type `element_id`, dispatching to the writer's bulk method for that type.
fn write_scalar_array<W: ValueWriter>(
  element_id: Uuid,
  value: &Value,
  writer: &mut W,
) -> Result<()> {
  macro_rules! write_array_of {
    ($variant:ident, $write:ident) => {{
      match value {
        Value::$variant(items) => writer.$write(items),
        other => err(format!(
          concat!("expected ", stringify!($variant), ", got {}"),
          other
        )),
      }
    }};
  }
  if element_id == *ty::BOOLEAN_ID {
    write_array_of!(ArrayBoolean, write_bool_array)
  } else if element_id == *ty::U8_ID {
    write_array_of!(ArrayU8, write_u8_array)
  } else if element_id == *ty::U16_ID {
    write_array_of!(ArrayU16, write_u16_array)
  } else if element_id == *ty::U32_ID {
    write_array_of!(ArrayU32, write_u32_array)
  } else if element_id == *ty::U64_ID {
    write_array_of!(ArrayU64, write_u64_array)
  } else if element_id == *ty::I8_ID {
    write_array_of!(ArrayI8, write_i8_array)
  } else if element_id == *ty::I16_ID {
    write_array_of!(ArrayI16, write_i16_array)
  } else if element_id == *ty::I32_ID {
    write_array_of!(ArrayI32, write_i32_array)
  } else if element_id == *ty::I64_ID {
    write_array_of!(ArrayI64, write_i64_array)
  } else if element_id == *ty::F32_ID {
    write_array_of!(ArrayF32, write_f32_array)
  } else if element_id == *ty::F64_ID {
    write_array_of!(ArrayF64, write_f64_array)
  } else if element_id == *ty::STRING_ID {
    write_array_of!(ArrayString, write_string_array)
  } else {
    err(format!(
      "array element type {element_id} is neither a registered type nor a supported scalar"
    ))
  }
}

/// Read an array whose element type is `element_id` (the counterpart of
/// [`write_array`]).
fn read_array<R: ValueReader>(
  element_id: Uuid,
  registry: &TypeRegistry,
  reader: &mut R,
) -> Result<Value> {
  match registry.get(&element_id) {
    Some(element_ty) => {
      let low::TypeKind::Structure(structure) = &element_ty.kind else {
        return err(format!(
          "array element type {element_id} is not a structure"
        ));
      };
      let len = reader.enter_struct_array(element_id)?;
      let mut elements = Vec::with_capacity(len);
      for _ in 0..len {
        reader.enter_struct_element(structure.fields.len())?;
        elements.push(StructureWithoutId {
          fields: read_fields(structure, registry, reader)?,
        });
      }
      Ok(Value::ArrayStructure {
        id: element_id,
        elements,
      })
    }
    None => read_scalar_array(element_id, reader),
  }
}

/// Read a scalar array whose elements are of the well-known type `element_id`,
/// dispatching to the reader's bulk method for that type.
fn read_scalar_array<R: ValueReader>(element_id: Uuid, reader: &mut R) -> Result<Value> {
  macro_rules! read_array_of {
    ($variant:ident, $read:ident) => {{
      Value::$variant(reader.$read()?)
    }};
  }
  Ok(if element_id == *ty::BOOLEAN_ID {
    read_array_of!(ArrayBoolean, read_bool_array)
  } else if element_id == *ty::U8_ID {
    read_array_of!(ArrayU8, read_u8_array)
  } else if element_id == *ty::U16_ID {
    read_array_of!(ArrayU16, read_u16_array)
  } else if element_id == *ty::U32_ID {
    read_array_of!(ArrayU32, read_u32_array)
  } else if element_id == *ty::U64_ID {
    read_array_of!(ArrayU64, read_u64_array)
  } else if element_id == *ty::I8_ID {
    read_array_of!(ArrayI8, read_i8_array)
  } else if element_id == *ty::I16_ID {
    read_array_of!(ArrayI16, read_i16_array)
  } else if element_id == *ty::I32_ID {
    read_array_of!(ArrayI32, read_i32_array)
  } else if element_id == *ty::I64_ID {
    read_array_of!(ArrayI64, read_i64_array)
  } else if element_id == *ty::F32_ID {
    read_array_of!(ArrayF32, read_f32_array)
  } else if element_id == *ty::F64_ID {
    read_array_of!(ArrayF64, read_f64_array)
  } else if element_id == *ty::STRING_ID {
    read_array_of!(ArrayString, read_string_array)
  } else {
    return err(format!(
      "array element type {element_id} is neither a registered type nor a supported scalar"
    ));
  })
}

fn write_scalar<W: ValueWriter>(id: Uuid, value: &Value, writer: &mut W) -> Result<()> {
  if id == *ty::UNIT_ID {
    expect_unit(value)?;
    writer.write_unit()
  } else if id == *ty::BOOLEAN_ID {
    writer.write_bool(as_bool(value)?)
  } else if id == *ty::U8_ID {
    writer.write_u8(as_u8(value)?)
  } else if id == *ty::U16_ID {
    writer.write_u16(as_u16(value)?)
  } else if id == *ty::U32_ID {
    writer.write_u32(as_u32(value)?)
  } else if id == *ty::U64_ID {
    writer.write_u64(as_u64(value)?)
  } else if id == *ty::I8_ID {
    writer.write_i8(as_i8(value)?)
  } else if id == *ty::I16_ID {
    writer.write_i16(as_i16(value)?)
  } else if id == *ty::I32_ID {
    writer.write_i32(as_i32(value)?)
  } else if id == *ty::I64_ID {
    writer.write_i64(as_i64(value)?)
  } else if id == *ty::F32_ID {
    writer.write_f32(as_f32(value)?)
  } else if id == *ty::F64_ID {
    writer.write_f64(as_f64(value)?)
  } else if id == *ty::STRING_ID {
    writer.write_string(as_str(value)?)
  } else {
    err(format!("type id {id} is not a supported primitive scalar"))
  }
}

fn read_scalar<R: ValueReader>(id: Uuid, reader: &mut R) -> Result<Value> {
  Ok(if id == *ty::UNIT_ID {
    reader.read_unit()?;
    Value::Unit
  } else if id == *ty::BOOLEAN_ID {
    Value::Boolean(reader.read_bool()?)
  } else if id == *ty::U8_ID {
    Value::U8(reader.read_u8()?)
  } else if id == *ty::U16_ID {
    Value::U16(reader.read_u16()?)
  } else if id == *ty::U32_ID {
    Value::U32(reader.read_u32()?)
  } else if id == *ty::U64_ID {
    Value::U64(reader.read_u64()?)
  } else if id == *ty::I8_ID {
    Value::I8(reader.read_i8()?)
  } else if id == *ty::I16_ID {
    Value::I16(reader.read_i16()?)
  } else if id == *ty::I32_ID {
    Value::I32(reader.read_i32()?)
  } else if id == *ty::I64_ID {
    Value::I64(reader.read_i64()?)
  } else if id == *ty::F32_ID {
    Value::F32(reader.read_f32()?)
  } else if id == *ty::F64_ID {
    Value::F64(reader.read_f64()?)
  } else if id == *ty::STRING_ID {
    Value::String(reader.read_string()?)
  } else {
    return err(format!("type id {id} is not a supported primitive scalar"));
  })
}

fn expect_unit(value: &Value) -> Result<()> {
  match value {
    Value::Unit => Ok(()),
    other => err(format!("expected unit, got {other}")),
  }
}

macro_rules! accessor {
  ($name:ident, $variant:ident, $ty:ty) => {
    fn $name(value: &Value) -> Result<$ty> {
      match value {
        Value::$variant(v) => Ok(*v),
        other => err(format!(
          concat!("expected ", stringify!($variant), ", got {}"),
          other
        )),
      }
    }
  };
}
accessor!(as_bool, Boolean, bool);
accessor!(as_u8, U8, u8);
accessor!(as_u16, U16, u16);
accessor!(as_u32, U32, u32);
accessor!(as_u64, U64, u64);
accessor!(as_i8, I8, i8);
accessor!(as_i16, I16, i16);
accessor!(as_i32, I32, i32);
accessor!(as_i64, I64, i64);
accessor!(as_f32, F32, f32);
accessor!(as_f64, F64, f64);

fn as_str(value: &Value) -> Result<&str> {
  match value {
    Value::String(s) => Ok(s),
    other => err(format!("expected String, got {other}")),
  }
}
