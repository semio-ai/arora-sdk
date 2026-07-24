use std::collections::HashSet;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::module::low::TypeRef;
use crate::value::{
  ConversionError, Enumeration as ValueEnumeration, Structure as ValueStructure,
  StructureField as ValueStructureField, Value,
};
use crate::{keyvalue::KeyValue, ty};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct StructureField {
  pub name: String,
  #[serde(rename = "type")]
  pub type_ref: TypeRef,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Structure {
  pub fields: IndexMap<Uuid, StructureField>,
}

impl Structure {
  pub fn type_dependencies(&self) -> HashSet<Uuid> {
    let mut deps = HashSet::new();
    for value in self.fields.values() {
      deps.extend(value.type_ref.type_dependencies());
    }
    deps
  }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct EnumerationValue {
  pub name: String,
  #[serde(rename = "type")]
  pub type_ref: TypeRef,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Enumeration {
  pub values: IndexMap<Uuid, EnumerationValue>,
}

impl Enumeration {
  pub fn type_dependencies(&self) -> HashSet<Uuid> {
    let mut deps = HashSet::new();
    for value in self.values.values() {
      deps.extend(value.type_ref.type_dependencies());
    }
    deps
  }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum TypeKind {
  Structure(Structure),
  Enumeration(Enumeration),
  Primitive(TypeRef),
}

impl TypeKind {
  pub fn type_dependencies(&self) -> HashSet<Uuid> {
    match self {
      Self::Structure(s) => s.type_dependencies(),
      Self::Enumeration(e) => e.type_dependencies(),
      Self::Primitive(_) => HashSet::new(),
    }
  }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Type {
  pub name: String,
  pub id: Uuid,
  pub description: String,
  pub kind: TypeKind,
}

impl Type {
  pub fn type_dependencies(&self) -> HashSet<Uuid> {
    self.kind.type_dependencies()
  }

  /// Builds a schema-aligned default [`Value`] for this type.
  ///
  /// Note: this does not recurse into nested user-defined struct/enum fields.
  /// For a field whose `TypeRef` points at a custom (non-well-known) type, it
  /// emits a minimal shell `Value::Structure { id, fields: vec![] }` rather than
  /// populating that nested type's fields. As a result, a default generated for
  /// a structure that has required custom-typed fields may not pass
  /// [`Type::validate`].
  pub fn default_value(&self) -> Value {
    default_value(self)
  }

  pub fn validate(&self, value: &Value) -> Result<(), ConversionError> {
    validate(value, self)
  }
}

/// Builds a schema-aligned default [`Value`] for `ty`.
///
/// Note: this does not recurse into nested user-defined struct/enum fields. For
/// a field whose `TypeRef` points at a custom (non-well-known) type, it emits a
/// minimal shell `Value::Structure { id, fields: vec![] }` rather than
/// populating that nested type's fields. As a result, a default generated for a
/// structure that has required custom-typed fields may not pass [`validate`].
pub fn default_value(ty: &Type) -> Value {
  match &ty.kind {
    TypeKind::Primitive(type_ref) => default_value_for_type_ref(type_ref),
    TypeKind::Structure(structure) => Value::Structure(ValueStructure {
      id: ty.id,
      fields: structure
        .fields
        .iter()
        .map(|(field_id, field)| ValueStructureField {
          id: *field_id,
          value: Box::new(default_value_for_type_ref(&field.type_ref)),
        })
        .collect(),
    }),
    TypeKind::Enumeration(enumeration) => {
      // Fields are ordered (IndexMap preserves declared order), so the default
      // uses the first declared variant.
      if let Some((variant_id, variant)) = enumeration.values.first() {
        Value::Enumeration(ValueEnumeration {
          id: ty.id,
          variant_id: *variant_id,
          value: Box::new(default_value_for_type_ref(&variant.type_ref)),
        })
      } else {
        Value::Enumeration(ValueEnumeration {
          id: ty.id,
          variant_id: Uuid::nil(),
          value: Box::new(Value::Unit),
        })
      }
    }
  }
}

pub fn validate(value: &Value, ty: &Type) -> Result<(), ConversionError> {
  match &ty.kind {
    TypeKind::Primitive(type_ref) => validate_type_ref(value, type_ref),
    TypeKind::Structure(structure) => {
      let Value::Structure(actual) = value else {
        return Err(validation_error("expected structure value"));
      };
      if actual.id != ty.id {
        return Err(validation_error("structure type id does not match schema"));
      }

      let expected_ids: HashSet<Uuid> = structure.fields.keys().copied().collect();
      let actual_ids: HashSet<Uuid> = actual.fields.iter().map(|field| field.id).collect();

      // The set check enforces missing/extra IDs; the length check additionally
      // catches duplicate field IDs in `actual.fields`.
      if expected_ids != actual_ids || actual.fields.len() != expected_ids.len() {
        return Err(validation_error(
          "structure fields do not match schema (missing, extra, or duplicated field id)",
        ));
      }

      for actual_field in &actual.fields {
        let expected_field = structure.fields.get(&actual_field.id).expect(
          "field id should exist after expected/actual field-id set equality check in validate()",
        );
        validate_type_ref(actual_field.value.as_ref(), &expected_field.type_ref)?;
      }

      Ok(())
    }
    TypeKind::Enumeration(enumeration) => {
      let Value::Enumeration(actual) = value else {
        return Err(validation_error("expected enumeration value"));
      };
      if actual.id != ty.id {
        return Err(validation_error(
          "enumeration type id does not match schema",
        ));
      }
      let Some(expected_variant) = enumeration.values.get(&actual.variant_id) else {
        return Err(validation_error(
          "enumeration variant id does not exist in schema",
        ));
      };
      validate_type_ref(actual.value.as_ref(), &expected_variant.type_ref)
    }
  }
}

fn default_value_for_type_ref(type_ref: &TypeRef) -> Value {
  match type_ref {
    TypeRef::Scalar { id } => default_value_for_scalar_type_id(*id),
    TypeRef::Array { id } => default_value_for_array_element_type_id(*id),
    TypeRef::Map { .. } => Value::KeyValue(KeyValue::default()),
  }
}

fn default_value_for_scalar_type_id(id: Uuid) -> Value {
  if id == *ty::UNIT_ID {
    Value::Unit
  } else if id == *ty::BOOLEAN_ID {
    Value::Boolean(false)
  } else if id == *ty::I8_ID {
    Value::I8(0)
  } else if id == *ty::I16_ID {
    Value::I16(0)
  } else if id == *ty::I32_ID {
    Value::I32(0)
  } else if id == *ty::I64_ID {
    Value::I64(0)
  } else if id == *ty::U8_ID {
    Value::U8(0)
  } else if id == *ty::U16_ID {
    Value::U16(0)
  } else if id == *ty::U32_ID {
    Value::U32(0)
  } else if id == *ty::U64_ID {
    Value::U64(0)
  } else if id == *ty::F32_ID {
    Value::F32(0.0)
  } else if id == *ty::F64_ID {
    Value::F64(0.0)
  } else if id == *ty::STRING_ID {
    Value::String(String::new())
  } else if id == *ty::OPTION_ID {
    Value::Option(None)
  } else if id == *ty::ARRAY_BOOLEAN_ID {
    Value::ArrayBoolean(vec![])
  } else if id == *ty::ARRAY_U8_ID {
    Value::ArrayU8(vec![])
  } else if id == *ty::ARRAY_U16_ID {
    Value::ArrayU16(vec![])
  } else if id == *ty::ARRAY_U32_ID {
    Value::ArrayU32(vec![])
  } else if id == *ty::ARRAY_U64_ID {
    Value::ArrayU64(vec![])
  } else if id == *ty::ARRAY_I8_ID {
    Value::ArrayI8(vec![])
  } else if id == *ty::ARRAY_I16_ID {
    Value::ArrayI16(vec![])
  } else if id == *ty::ARRAY_I32_ID {
    Value::ArrayI32(vec![])
  } else if id == *ty::ARRAY_I64_ID {
    Value::ArrayI64(vec![])
  } else if id == *ty::ARRAY_F32_ID {
    Value::ArrayF32(vec![])
  } else if id == *ty::ARRAY_F64_ID {
    Value::ArrayF64(vec![])
  } else if id == *ty::ARRAY_STRING_ID {
    Value::ArrayString(vec![])
  } else if id == *ty::ARRAY_VALUE_ID {
    Value::ArrayValue(vec![])
  } else if id == *ty::KEY_VALUE_ID {
    Value::KeyValue(KeyValue::default())
  } else if id == *ty::UUID_ID {
    Value::Uuid(Uuid::nil())
  } else {
    // For custom schema IDs (typically user-defined structure/enumeration IDs),
    // preserve the type UUID in a minimal shell value.
    Value::Structure(ValueStructure { id, fields: vec![] })
  }
}

fn default_value_for_array_element_type_id(id: Uuid) -> Value {
  if id == *ty::BOOLEAN_ID {
    Value::ArrayBoolean(vec![])
  } else if id == *ty::U8_ID {
    Value::ArrayU8(vec![])
  } else if id == *ty::U16_ID {
    Value::ArrayU16(vec![])
  } else if id == *ty::U32_ID {
    Value::ArrayU32(vec![])
  } else if id == *ty::U64_ID {
    Value::ArrayU64(vec![])
  } else if id == *ty::I8_ID {
    Value::ArrayI8(vec![])
  } else if id == *ty::I16_ID {
    Value::ArrayI16(vec![])
  } else if id == *ty::I32_ID {
    Value::ArrayI32(vec![])
  } else if id == *ty::I64_ID {
    Value::ArrayI64(vec![])
  } else if id == *ty::F32_ID {
    Value::ArrayF32(vec![])
  } else if id == *ty::F64_ID {
    Value::ArrayF64(vec![])
  } else if id == *ty::STRING_ID {
    Value::ArrayString(vec![])
  } else {
    Value::ArrayStructure {
      id,
      elements: vec![],
    }
  }
}

fn validate_type_ref(value: &Value, type_ref: &TypeRef) -> Result<(), ConversionError> {
  match type_ref {
    TypeRef::Scalar { id } => validate_scalar_type_id(value, *id),
    TypeRef::Array { id } => validate_array_element_type_id(value, *id),
    TypeRef::Map { .. } => {
      if matches!(value, Value::KeyValue(_)) {
        Ok(())
      } else {
        Err(validation_error("expected key/value map value"))
      }
    }
  }
}

fn validate_scalar_type_id(value: &Value, id: Uuid) -> Result<(), ConversionError> {
  let valid = if id == *ty::UNIT_ID {
    matches!(value, Value::Unit)
  } else if id == *ty::BOOLEAN_ID {
    matches!(value, Value::Boolean(_))
  } else if id == *ty::I8_ID {
    matches!(value, Value::I8(_))
  } else if id == *ty::I16_ID {
    matches!(value, Value::I16(_))
  } else if id == *ty::I32_ID {
    matches!(value, Value::I32(_))
  } else if id == *ty::I64_ID {
    matches!(value, Value::I64(_))
  } else if id == *ty::U8_ID {
    matches!(value, Value::U8(_))
  } else if id == *ty::U16_ID {
    matches!(value, Value::U16(_))
  } else if id == *ty::U32_ID {
    matches!(value, Value::U32(_))
  } else if id == *ty::U64_ID {
    matches!(value, Value::U64(_))
  } else if id == *ty::F32_ID {
    matches!(value, Value::F32(_))
  } else if id == *ty::F64_ID {
    matches!(value, Value::F64(_))
  } else if id == *ty::STRING_ID {
    matches!(value, Value::String(_))
  } else if id == *ty::OPTION_ID {
    matches!(value, Value::Option(_))
  } else if id == *ty::ARRAY_BOOLEAN_ID {
    matches!(value, Value::ArrayBoolean(_))
  } else if id == *ty::ARRAY_U8_ID {
    matches!(value, Value::ArrayU8(_))
  } else if id == *ty::ARRAY_U16_ID {
    matches!(value, Value::ArrayU16(_))
  } else if id == *ty::ARRAY_U32_ID {
    matches!(value, Value::ArrayU32(_))
  } else if id == *ty::ARRAY_U64_ID {
    matches!(value, Value::ArrayU64(_))
  } else if id == *ty::ARRAY_I8_ID {
    matches!(value, Value::ArrayI8(_))
  } else if id == *ty::ARRAY_I16_ID {
    matches!(value, Value::ArrayI16(_))
  } else if id == *ty::ARRAY_I32_ID {
    matches!(value, Value::ArrayI32(_))
  } else if id == *ty::ARRAY_I64_ID {
    matches!(value, Value::ArrayI64(_))
  } else if id == *ty::ARRAY_F32_ID {
    matches!(value, Value::ArrayF32(_))
  } else if id == *ty::ARRAY_F64_ID {
    matches!(value, Value::ArrayF64(_))
  } else if id == *ty::ARRAY_STRING_ID {
    matches!(value, Value::ArrayString(_))
  } else if id == *ty::ARRAY_VALUE_ID {
    matches!(value, Value::ArrayValue(_))
  } else if id == *ty::KEY_VALUE_ID {
    matches!(value, Value::KeyValue(_))
  } else if id == *ty::UUID_ID {
    matches!(value, Value::Uuid(_))
  } else {
    matches!(value, Value::Structure(s) if s.id == id)
      || matches!(value, Value::Enumeration(e) if e.id == id)
  };

  if valid {
    Ok(())
  } else {
    Err(validation_error(
      "value does not match the scalar type reference schema",
    ))
  }
}

fn validate_array_element_type_id(value: &Value, id: Uuid) -> Result<(), ConversionError> {
  let valid = if id == *ty::BOOLEAN_ID {
    matches!(value, Value::ArrayBoolean(_))
  } else if id == *ty::U8_ID {
    matches!(value, Value::ArrayU8(_))
  } else if id == *ty::U16_ID {
    matches!(value, Value::ArrayU16(_))
  } else if id == *ty::U32_ID {
    matches!(value, Value::ArrayU32(_))
  } else if id == *ty::U64_ID {
    matches!(value, Value::ArrayU64(_))
  } else if id == *ty::I8_ID {
    matches!(value, Value::ArrayI8(_))
  } else if id == *ty::I16_ID {
    matches!(value, Value::ArrayI16(_))
  } else if id == *ty::I32_ID {
    matches!(value, Value::ArrayI32(_))
  } else if id == *ty::I64_ID {
    matches!(value, Value::ArrayI64(_))
  } else if id == *ty::F32_ID {
    matches!(value, Value::ArrayF32(_))
  } else if id == *ty::F64_ID {
    matches!(value, Value::ArrayF64(_))
  } else if id == *ty::STRING_ID {
    matches!(value, Value::ArrayString(_))
  } else {
    matches!(value, Value::ArrayStructure { id: value_id, .. } if *value_id == id)
      || matches!(value, Value::ArrayEnumeration { id: value_id, .. } if *value_id == id)
  };

  if valid {
    Ok(())
  } else {
    Err(validation_error(
      "value does not match the array type reference schema",
    ))
  }
}

fn validation_error(message: &str) -> ConversionError {
  ConversionError {
    message: message.to_string(),
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use indexmap::IndexMap;
  use std::collections::HashMap;

  #[test]
  fn default_value_builds_structure_with_field_ids_from_schema() {
    let ty_id = Uuid::new_v4();
    let field_bool_id = Uuid::new_v4();
    let field_i32_id = Uuid::new_v4();
    let ty = Type {
      name: "Sample".to_string(),
      id: ty_id,
      description: "sample structure".to_string(),
      kind: TypeKind::Structure(Structure {
        fields: IndexMap::from([
          (
            field_bool_id,
            StructureField {
              name: "enabled".to_string(),
              type_ref: TypeRef::Scalar {
                id: *ty::BOOLEAN_ID,
              },
            },
          ),
          (
            field_i32_id,
            StructureField {
              name: "count".to_string(),
              type_ref: TypeRef::Scalar { id: *ty::I32_ID },
            },
          ),
        ]),
      }),
    };

    let Value::Structure(value) = default_value(&ty) else {
      panic!("expected structure default value");
    };

    assert_eq!(value.id, ty_id);
    let fields: HashMap<Uuid, Value> = value
      .fields
      .into_iter()
      .map(|field| (field.id, *field.value))
      .collect();
    assert_eq!(fields.get(&field_bool_id), Some(&Value::Boolean(false)));
    assert_eq!(fields.get(&field_i32_id), Some(&Value::I32(0)));
  }

  #[test]
  fn validate_accepts_matching_structure_value() {
    let ty = Type {
      name: "OnlyBool".to_string(),
      id: Uuid::new_v4(),
      description: "structure with one bool".to_string(),
      kind: TypeKind::Structure(Structure {
        fields: IndexMap::from([(
          Uuid::new_v4(),
          StructureField {
            name: "enabled".to_string(),
            type_ref: TypeRef::Scalar {
              id: *ty::BOOLEAN_ID,
            },
          },
        )]),
      }),
    };

    let value = default_value(&ty);
    assert!(validate(&value, &ty).is_ok());
    assert!(ty.validate(&value).is_ok());
  }

  #[test]
  fn validate_rejects_structure_with_wrong_field_type() {
    let field_id = Uuid::new_v4();
    let ty = Type {
      name: "OnlyBool".to_string(),
      id: Uuid::new_v4(),
      description: "structure with one bool".to_string(),
      kind: TypeKind::Structure(Structure {
        fields: IndexMap::from([(
          field_id,
          StructureField {
            name: "enabled".to_string(),
            type_ref: TypeRef::Scalar {
              id: *ty::BOOLEAN_ID,
            },
          },
        )]),
      }),
    };
    let value = Value::Structure(ValueStructure {
      id: ty.id,
      fields: vec![ValueStructureField {
        id: field_id,
        value: Box::new(Value::I32(10)),
      }],
    });

    assert!(validate(&value, &ty).is_err());
  }
}
