//! Seeding the serde bridge with a declared [`low::Type`].
//!
//! The plain [`bridge`](super::bridge) mints struct and enum ids by hashing
//! field and type *names* — it has no type to consult, so it re-derives the same
//! ids on the way back. Seeded with a [`low::Type`], the ids come from the type
//! instead:
//!
//! - [`to_value_seeded`] produces a [`Value`] carrying the type's *declared*
//!   ids — ready for the type-directed [`walk`](super::walk), or for a remote
//!   that knows the type by id rather than by name;
//! - [`from_value_seeded`] reads such a [`Value`] back into a Rust type — which
//!   the plain reader cannot, since it only recognises name-hash ids.
//!
//! It composes the bridge with a type-directed id remap: **serde supplies the
//! structure, the type supplies the ids**. The bridge's name-hash `Value` is the
//! intermediate, so a field's Rust name must match its declared name — which the
//! [`AroraType`](crate::AroraType) derive guarantees, since it takes both from
//! the same Rust definition.
//!
//! First cut: structures of primitive, string and nested-structure fields — the
//! shape the derive and the walk support. Enumerations, arrays and maps error
//! explicitly.

use serde::de::DeserializeOwned;
use serde::Serialize;

use super::{bridge, Error, Result};
use crate::gen_uuid_from_str;
use crate::module::low::TypeRef;
use crate::ty::{self, low, TypeRegistry};
use crate::value::{Structure, StructureField, Value};

/// Convert a Rust value into a [`Value`] whose ids are `ty`'s declared ids.
pub fn to_value_seeded<T: Serialize + ?Sized>(
  value: &T,
  ty: &low::Type,
  registry: &TypeRegistry,
) -> Result<Value> {
  impose_ids(bridge::to_value(value)?, ty, registry)
}

/// Read a [`Value`] carrying `ty`'s declared ids back into a Rust value.
pub fn from_value_seeded<T: DeserializeOwned>(
  value: Value,
  ty: &low::Type,
  registry: &TypeRegistry,
) -> Result<T> {
  bridge::from_value(strip_ids(value, ty, registry)?)
}

/// Rewrite a name-hashed [`Value`] (as the plain bridge produces) so each
/// structure carries `ty`'s declared ids.
fn impose_ids(value: Value, ty: &low::Type, registry: &TypeRegistry) -> Result<Value> {
  match &ty.kind {
    low::TypeKind::Primitive(type_ref) => impose_by_ref(value, type_ref, registry),
    low::TypeKind::Structure(structure) => {
      let actual = expect_structure(value, ty)?;
      let mut fields = Vec::with_capacity(structure.fields.len());
      for (field_id, field) in &structure.fields {
        // The plain bridge keyed this field by hash(name); find it there and
        // re-key it to the declared id.
        let value = take_field(&actual, gen_uuid_from_str(&field.name), &field.name, ty)?;
        fields.push(StructureField {
          id: *field_id,
          value: Box::new(impose_by_ref(value, &field.type_ref, registry)?),
        });
      }
      Ok(Value::Structure(Structure { id: ty.id, fields }))
    }
    low::TypeKind::Enumeration(_) => Err(Error::new(
      "enumeration types are not supported by the seeded bridge yet",
    )),
  }
}

/// Rewrite a declared-id [`Value`] back to the name-hashed form the plain bridge
/// reads (field ids become hash(name), which is what its deserializer matches).
fn strip_ids(value: Value, ty: &low::Type, registry: &TypeRegistry) -> Result<Value> {
  match &ty.kind {
    low::TypeKind::Primitive(type_ref) => strip_by_ref(value, type_ref, registry),
    low::TypeKind::Structure(structure) => {
      let actual = expect_structure(value, ty)?;
      let mut fields = Vec::with_capacity(structure.fields.len());
      for (field_id, field) in &structure.fields {
        let value = take_field(&actual, *field_id, &field.name, ty)?;
        fields.push(StructureField {
          id: gen_uuid_from_str(&field.name),
          value: Box::new(strip_by_ref(value, &field.type_ref, registry)?),
        });
      }
      Ok(Value::Structure(Structure {
        id: gen_uuid_from_str(&ty.name),
        fields,
      }))
    }
    low::TypeKind::Enumeration(_) => Err(Error::new(
      "enumeration types are not supported by the seeded bridge yet",
    )),
  }
}

fn impose_by_ref(value: Value, type_ref: &TypeRef, registry: &TypeRegistry) -> Result<Value> {
  remap_by_ref(value, type_ref, registry, impose_ids)
}

fn strip_by_ref(value: Value, type_ref: &TypeRef, registry: &TypeRegistry) -> Result<Value> {
  remap_by_ref(value, type_ref, registry, strip_ids)
}

/// A field value whose type is named by `type_ref`: a primitive carries no id to
/// remap and passes through; any other id is resolved in `registry` and its
/// structure remapped by `recurse`.
fn remap_by_ref(
  value: Value,
  type_ref: &TypeRef,
  registry: &TypeRegistry,
  recurse: fn(Value, &low::Type, &TypeRegistry) -> Result<Value>,
) -> Result<Value> {
  match type_ref {
    TypeRef::Scalar { id } if ty::PRIMITIVE_IDS.contains(id) => Ok(value),
    TypeRef::Scalar { id } => {
      let nested = registry
        .get(id)
        .ok_or_else(|| Error::new(format!("type {id} not found in the registry")))?;
      recurse(value, nested, registry)
    }
    TypeRef::Array { .. } => Err(Error::new(
      "array types are not supported by the seeded bridge yet",
    )),
    TypeRef::Map { .. } => Err(Error::new(
      "map types are not supported by the seeded bridge yet",
    )),
  }
}

fn expect_structure(value: Value, ty: &low::Type) -> Result<Structure> {
  match value {
    Value::Structure(s) => Ok(s),
    other => Err(Error::new(format!(
      "expected a structure value for type {}, got {other}",
      ty.name
    ))),
  }
}

/// Take the value of the field carrying `id` out of `structure`.
fn take_field(structure: &Structure, id: uuid::Uuid, name: &str, ty: &low::Type) -> Result<Value> {
  structure
    .fields
    .iter()
    .find(|f| f.id == id)
    .map(|f| (*f.value).clone())
    .ok_or_else(|| Error::new(format!("missing field `{name}` for type {}", ty.name)))
}

#[cfg(all(test, feature = "derive"))]
mod tests {
  use super::*;
  use crate::AroraType;
  use serde::{Deserialize, Serialize};

  const POINT_ID: &str = "0a0a0a0a-0000-4000-8000-000000000001";
  const POINT_X_ID: &str = "0a0a0a0a-0000-4000-8000-000000000002";

  #[derive(Serialize, Deserialize, AroraType, Debug, PartialEq)]
  #[arora(id = "0a0a0a0a-0000-4000-8000-000000000001")]
  struct Point {
    // An explicit id no name-hash would produce.
    #[arora(id = "0a0a0a0a-0000-4000-8000-000000000002")]
    x: f64,
    y: f64,
  }

  #[derive(Serialize, Deserialize, AroraType, Debug, PartialEq)]
  struct Line {
    from: Point,
    to: Point,
  }

  #[test]
  fn seeded_value_carries_the_types_declared_ids() {
    let (ty, registry) = Point::arora_type_with_registry();
    let value = to_value_seeded(&Point { x: 1.0, y: 2.0 }, &ty, &registry).unwrap();

    let Value::Structure(s) = &value else {
      panic!("expected a structure");
    };
    assert_eq!(s.id, crate::Uuid::parse_str(POINT_ID).unwrap());
    // The x field's id is the explicit one, not hash("x").
    assert_eq!(s.fields[0].id, crate::Uuid::parse_str(POINT_X_ID).unwrap());
    assert_ne!(s.fields[0].id, gen_uuid_from_str("x"));
  }

  #[test]
  fn plain_reader_cannot_read_seeded_ids_but_seeded_reader_can() {
    let (ty, registry) = Point::arora_type_with_registry();
    let point = Point { x: 1.0, y: 2.0 };
    let value = to_value_seeded(&point, &ty, &registry).unwrap();

    // The explicit x id is not hash("x"), so the plain (un-seeded) reader drops
    // that field and fails.
    assert!(bridge::from_value::<Point>(value.clone()).is_err());

    // The seeded reader resolves ids through the type and round-trips.
    let back: Point = from_value_seeded(value, &ty, &registry).unwrap();
    assert_eq!(back, point);
  }

  #[test]
  fn nested_structs_round_trip_seeded() {
    let (ty, registry) = Line::arora_type_with_registry();
    let line = Line {
      from: Point { x: 0.0, y: 0.0 },
      to: Point { x: 3.0, y: 4.0 },
    };
    let value = to_value_seeded(&line, &ty, &registry).unwrap();
    let back: Line = from_value_seeded(value, &ty, &registry).unwrap();
    assert_eq!(back, line);
  }
}
