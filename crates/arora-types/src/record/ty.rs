//! Type expressions for versioned type records: primitives plus references to
//! other type records, in unfrozen (version-required) and frozen
//! (version-pinned) form.
//!
//! The serde shapes mirror semio-record's `ty` module exactly — these types are
//! the wire format of the frozen records the Semio store consumes (see the
//! golden tests in [`super::wire_tests`]).

use std::str::FromStr;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::freeze::{Freeze, Resolver};
use super::reference::{FrozenReference, UnfrozenReference, Version};
use crate::module::low::TypeRef;
use uuid::Uuid;

/// A built-in type: scalars and their array forms.
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash)]
#[serde(rename = "Primitive_Kind", rename_all = "camelCase")]
pub enum PrimitiveKind {
  Unit,
  Boolean,
  U8,
  U16,
  U32,
  U64,
  I8,
  I16,
  I32,
  I64,
  F32,
  F64,
  String,
  ArrayBoolean,
  ArrayU8,
  ArrayU16,
  ArrayU32,
  ArrayU64,
  ArrayI8,
  ArrayI16,
  ArrayI32,
  ArrayI64,
  ArrayF32,
  ArrayF64,
  ArrayString,
}

impl PrimitiveKind {
  pub fn is_array(&self) -> bool {
    matches!(
      self,
      PrimitiveKind::ArrayBoolean
        | PrimitiveKind::ArrayU8
        | PrimitiveKind::ArrayU16
        | PrimitiveKind::ArrayU32
        | PrimitiveKind::ArrayU64
        | PrimitiveKind::ArrayI8
        | PrimitiveKind::ArrayI16
        | PrimitiveKind::ArrayI32
        | PrimitiveKind::ArrayI64
        | PrimitiveKind::ArrayF32
        | PrimitiveKind::ArrayF64
        | PrimitiveKind::ArrayString
    )
  }

  pub fn is_scalar(&self) -> bool {
    !self.is_array()
  }

  /// The primitive a well-known type id names — a scalar primitive, or the
  /// array form of one — or `None` for any other id: a record's, or a
  /// well-known type that is not a primitive (a uuid, a dynamic value).
  pub fn from_id(id: &Uuid) -> Option<PrimitiveKind> {
    use crate::ty;
    use PrimitiveKind::*;
    let table = [
      (*ty::UNIT_ID, Unit),
      (*ty::BOOLEAN_ID, Boolean),
      (*ty::U8_ID, U8),
      (*ty::U16_ID, U16),
      (*ty::U32_ID, U32),
      (*ty::U64_ID, U64),
      (*ty::I8_ID, I8),
      (*ty::I16_ID, I16),
      (*ty::I32_ID, I32),
      (*ty::I64_ID, I64),
      (*ty::F32_ID, F32),
      (*ty::F64_ID, F64),
      (*ty::STRING_ID, String),
      (*ty::ARRAY_BOOLEAN_ID, ArrayBoolean),
      (*ty::ARRAY_U8_ID, ArrayU8),
      (*ty::ARRAY_U16_ID, ArrayU16),
      (*ty::ARRAY_U32_ID, ArrayU32),
      (*ty::ARRAY_U64_ID, ArrayU64),
      (*ty::ARRAY_I8_ID, ArrayI8),
      (*ty::ARRAY_I16_ID, ArrayI16),
      (*ty::ARRAY_I32_ID, ArrayI32),
      (*ty::ARRAY_I64_ID, ArrayI64),
      (*ty::ARRAY_F32_ID, ArrayF32),
      (*ty::ARRAY_F64_ID, ArrayF64),
      (*ty::ARRAY_STRING_ID, ArrayString),
    ];
    table
      .iter()
      .find(|(known, _)| known == id)
      .map(|(_, kind)| *kind)
  }

  /// The array form of a scalar primitive, or `None` for one that has none:
  /// unit, or an array already.
  pub fn array_of(self) -> Option<PrimitiveKind> {
    use PrimitiveKind::*;
    Some(match self {
      Boolean => ArrayBoolean,
      U8 => ArrayU8,
      U16 => ArrayU16,
      U32 => ArrayU32,
      U64 => ArrayU64,
      I8 => ArrayI8,
      I16 => ArrayI16,
      I32 => ArrayI32,
      I64 => ArrayI64,
      F32 => ArrayF32,
      F64 => ArrayF64,
      String => ArrayString,
      _ => return None,
    })
  }
}

impl std::fmt::Display for PrimitiveKind {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    let s = match self {
      PrimitiveKind::Unit => "unit",
      PrimitiveKind::Boolean => "bool",
      PrimitiveKind::U8 => "u8",
      PrimitiveKind::U16 => "u16",
      PrimitiveKind::U32 => "u32",
      PrimitiveKind::U64 => "u64",
      PrimitiveKind::I8 => "i8",
      PrimitiveKind::I16 => "i16",
      PrimitiveKind::I32 => "i32",
      PrimitiveKind::I64 => "i64",
      PrimitiveKind::F32 => "f32",
      PrimitiveKind::F64 => "f64",
      PrimitiveKind::String => "str",
      PrimitiveKind::ArrayBoolean => "bool[]",
      PrimitiveKind::ArrayU8 => "u8[]",
      PrimitiveKind::ArrayU16 => "u16[]",
      PrimitiveKind::ArrayU32 => "u32[]",
      PrimitiveKind::ArrayU64 => "u64[]",
      PrimitiveKind::ArrayI8 => "i8[]",
      PrimitiveKind::ArrayI16 => "i16[]",
      PrimitiveKind::ArrayI32 => "i32[]",
      PrimitiveKind::ArrayI64 => "i64[]",
      PrimitiveKind::ArrayF32 => "f32[]",
      PrimitiveKind::ArrayF64 => "f64[]",
      PrimitiveKind::ArrayString => "str[]",
    };
    f.write_str(s)
  }
}

impl FromStr for PrimitiveKind {
  type Err = String;

  fn from_str(s: &str) -> Result<Self, Self::Err> {
    match s {
      "unit" => Ok(PrimitiveKind::Unit),
      "bool" => Ok(PrimitiveKind::Boolean),
      "u8" => Ok(PrimitiveKind::U8),
      "u16" => Ok(PrimitiveKind::U16),
      "u32" => Ok(PrimitiveKind::U32),
      "u64" => Ok(PrimitiveKind::U64),
      "i8" => Ok(PrimitiveKind::I8),
      "i16" => Ok(PrimitiveKind::I16),
      "i32" => Ok(PrimitiveKind::I32),
      "i64" => Ok(PrimitiveKind::I64),
      "f32" => Ok(PrimitiveKind::F32),
      "f64" => Ok(PrimitiveKind::F64),
      "str" => Ok(PrimitiveKind::String),
      "bool[]" => Ok(PrimitiveKind::ArrayBoolean),
      "u8[]" => Ok(PrimitiveKind::ArrayU8),
      "u16[]" => Ok(PrimitiveKind::ArrayU16),
      "u32[]" => Ok(PrimitiveKind::ArrayU32),
      "u64[]" => Ok(PrimitiveKind::ArrayU64),
      "i8[]" => Ok(PrimitiveKind::ArrayI8),
      "i16[]" => Ok(PrimitiveKind::ArrayI16),
      "i32[]" => Ok(PrimitiveKind::ArrayI32),
      "i64[]" => Ok(PrimitiveKind::ArrayI64),
      "f32[]" => Ok(PrimitiveKind::ArrayF32),
      "f64[]" => Ok(PrimitiveKind::ArrayF64),
      "str[]" => Ok(PrimitiveKind::ArrayString),
      _ => Err(format!("unknown primitive kind: {}", s)),
    }
  }
}

/// A primitive type expression.
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Primitive {
  pub kind: PrimitiveKind,
}

impl From<PrimitiveKind> for Primitive {
  fn from(kind: PrimitiveKind) -> Self {
    Self { kind }
  }
}

impl Primitive {
  pub const UNIT: Self = Self {
    kind: PrimitiveKind::Unit,
  };
  pub const BOOLEAN: Self = Self {
    kind: PrimitiveKind::Boolean,
  };
  pub const U8: Self = Self {
    kind: PrimitiveKind::U8,
  };
  pub const U16: Self = Self {
    kind: PrimitiveKind::U16,
  };
  pub const U32: Self = Self {
    kind: PrimitiveKind::U32,
  };
  pub const U64: Self = Self {
    kind: PrimitiveKind::U64,
  };
  pub const I8: Self = Self {
    kind: PrimitiveKind::I8,
  };
  pub const I16: Self = Self {
    kind: PrimitiveKind::I16,
  };
  pub const I32: Self = Self {
    kind: PrimitiveKind::I32,
  };
  pub const I64: Self = Self {
    kind: PrimitiveKind::I64,
  };
  pub const F32: Self = Self {
    kind: PrimitiveKind::F32,
  };
  pub const F64: Self = Self {
    kind: PrimitiveKind::F64,
  };
  pub const STRING: Self = Self {
    kind: PrimitiveKind::String,
  };
  pub const ARRAY_BOOLEAN: Self = Self {
    kind: PrimitiveKind::ArrayBoolean,
  };
  pub const ARRAY_U8: Self = Self {
    kind: PrimitiveKind::ArrayU8,
  };
  pub const ARRAY_U16: Self = Self {
    kind: PrimitiveKind::ArrayU16,
  };
  pub const ARRAY_U32: Self = Self {
    kind: PrimitiveKind::ArrayU32,
  };
  pub const ARRAY_U64: Self = Self {
    kind: PrimitiveKind::ArrayU64,
  };
  pub const ARRAY_I8: Self = Self {
    kind: PrimitiveKind::ArrayI8,
  };
  pub const ARRAY_I16: Self = Self {
    kind: PrimitiveKind::ArrayI16,
  };
  pub const ARRAY_I32: Self = Self {
    kind: PrimitiveKind::ArrayI32,
  };
  pub const ARRAY_I64: Self = Self {
    kind: PrimitiveKind::ArrayI64,
  };
  pub const ARRAY_F32: Self = Self {
    kind: PrimitiveKind::ArrayF32,
  };
  pub const ARRAY_F64: Self = Self {
    kind: PrimitiveKind::ArrayF64,
  };
  pub const ARRAY_STRING: Self = Self {
    kind: PrimitiveKind::ArrayString,
  };
}

impl Primitive {
  pub fn is_array(&self) -> bool {
    self.kind.is_array()
  }

  pub fn is_scalar(&self) -> bool {
    self.kind.is_scalar()
  }
}

impl std::fmt::Display for Primitive {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    self.kind.fmt(f)
  }
}

/// A scalar reference to another type record, version not yet pinned.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
#[serde(rename = "unfrozen_Scalar")]
pub struct UnfrozenScalar {
  pub reference: UnfrozenReference,
}

/// An array of another type record, version not yet pinned.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
#[serde(rename = "unfrozen_Array")]
pub struct UnfrozenArray {
  pub reference: UnfrozenReference,
}

/// An optional value of another type expression, versions not yet pinned: an
/// absent value, or a present one of type `element`.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
#[serde(rename = "unfrozen_Option")]
pub struct UnfrozenOption {
  pub element: Box<UnfrozenTy>,
}

/// A scalar reference to another type record, pinned to a concrete version.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
#[serde(rename = "frozen_Scalar")]
pub struct FrozenScalar {
  pub reference: FrozenReference,
}

/// An array of another type record, pinned to a concrete version.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
#[serde(rename = "frozen_Array")]
pub struct FrozenArray {
  pub reference: FrozenReference,
}

/// An optional value of another type expression, every version pinned: an
/// absent value, or a present one of type `element`.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
#[serde(rename = "frozen_Option")]
pub struct FrozenOption {
  pub element: Box<FrozenTy>,
}

/// A type expression whose record references are not yet version-pinned.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
#[serde(
  rename = "unfrozen_Type",
  rename_all = "camelCase",
  tag = "type",
  content = "value"
)]
pub enum UnfrozenTy {
  Primitive(Primitive),
  UnfrozenScalar(UnfrozenScalar),
  UnfrozenArray(UnfrozenArray),
  UnfrozenOption(UnfrozenOption),
}

impl From<PrimitiveKind> for UnfrozenTy {
  fn from(kind: PrimitiveKind) -> Self {
    Self::Primitive(Primitive::from(kind))
  }
}

impl UnfrozenTy {
  pub fn as_primitive(&self) -> Option<&Primitive> {
    match self {
      Self::Primitive(primitive) => Some(primitive),
      _ => None,
    }
  }
}

/// A type expression with every record reference pinned to a concrete version.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
#[serde(
  rename = "frozen_Type",
  rename_all = "camelCase",
  tag = "type",
  content = "value"
)]
pub enum FrozenTy {
  Primitive(Primitive),
  FrozenScalar(FrozenScalar),
  FrozenArray(FrozenArray),
  FrozenOption(FrozenOption),
}

impl From<PrimitiveKind> for FrozenTy {
  fn from(kind: PrimitiveKind) -> Self {
    Self::Primitive(Primitive::from(kind))
  }
}

impl FrozenTy {
  /// The frozen form of a header's type reference, each record it names
  /// pinned at `version`.
  ///
  /// - A scalar naming a primitive id is that primitive; naming any other id
  ///   — a record's, or a well-known type that is not a primitive (a uuid, a
  ///   dynamic value) — it is a reference to it.
  /// - An array of a scalar primitive is that primitive's array form; an
  ///   array of anything else is an array reference.
  /// - An optional is an optional of its element read as a scalar.
  ///
  /// `None` for a fixed-length array or a map, which a record cannot express.
  pub fn from_type_ref(type_ref: &TypeRef, version: &Version) -> Option<FrozenTy> {
    let reference = |id: &Uuid| FrozenReference {
      id: *id,
      version: version.clone(),
    };
    Some(match type_ref {
      TypeRef::Scalar { id } => match PrimitiveKind::from_id(id) {
        Some(kind) => FrozenTy::from(kind),
        None => FrozenTy::FrozenScalar(FrozenScalar {
          reference: reference(id),
        }),
      },
      TypeRef::Array { id } => match PrimitiveKind::from_id(id).and_then(PrimitiveKind::array_of) {
        Some(kind) => FrozenTy::from(kind),
        None => FrozenTy::FrozenArray(FrozenArray {
          reference: reference(id),
        }),
      },
      TypeRef::Option { id } => FrozenTy::FrozenOption(FrozenOption {
        element: Box::new(Self::from_type_ref(&TypeRef::Scalar { id: *id }, version)?),
      }),
      TypeRef::FixedArray { .. } | TypeRef::Map { .. } => return None,
    })
  }

  pub fn as_primitive(&self) -> Option<&Primitive> {
    match self {
      Self::Primitive(primitive) => Some(primitive),
      _ => None,
    }
  }

  pub fn is_primitive(&self) -> bool {
    matches!(self, Self::Primitive(_))
  }

  pub fn is_scalar(&self) -> bool {
    matches!(self, Self::FrozenScalar(_))
  }

  pub fn is_array(&self) -> bool {
    matches!(self, Self::FrozenArray(_))
  }

  pub fn is_option(&self) -> bool {
    matches!(self, Self::FrozenOption(_))
  }

  /// Collect the pinned record references this type expression depends on.
  pub fn dependencies<'a>(&'a self, set: &mut std::collections::HashSet<&'a FrozenReference>) {
    match self {
      Self::Primitive(_) => {}
      Self::FrozenScalar(scalar) => {
        set.insert(&scalar.reference);
      }
      Self::FrozenArray(array) => {
        set.insert(&array.reference);
      }
      Self::FrozenOption(option) => option.element.dependencies(set),
    }
  }

  pub fn as_scalar(&self) -> Option<&FrozenScalar> {
    match self {
      Self::FrozenScalar(scalar) => Some(scalar),
      _ => None,
    }
  }

  pub fn as_array(&self) -> Option<&FrozenArray> {
    match self {
      Self::FrozenArray(array) => Some(array),
      _ => None,
    }
  }

  pub fn as_option(&self) -> Option<&FrozenOption> {
    match self {
      Self::FrozenOption(option) => Some(option),
      _ => None,
    }
  }
}

#[async_trait]
impl<R: Resolver> Freeze<R> for UnfrozenScalar {
  type Frozen = FrozenScalar;

  async fn freeze(&self, resolver: &R) -> Result<Self::Frozen, R::Error> {
    Ok(FrozenScalar {
      reference: self.reference.freeze(resolver).await?,
    })
  }
}

#[async_trait]
impl<R: Resolver> Freeze<R> for UnfrozenArray {
  type Frozen = FrozenArray;

  async fn freeze(&self, resolver: &R) -> Result<Self::Frozen, R::Error> {
    Ok(FrozenArray {
      reference: self.reference.freeze(resolver).await?,
    })
  }
}

#[async_trait]
impl<R: Resolver> Freeze<R> for UnfrozenOption {
  type Frozen = FrozenOption;

  async fn freeze(&self, resolver: &R) -> Result<Self::Frozen, R::Error> {
    Ok(FrozenOption {
      element: Box::new(self.element.freeze(resolver).await?),
    })
  }
}

#[async_trait]
impl<R: Resolver> Freeze<R> for UnfrozenTy {
  type Frozen = FrozenTy;

  async fn freeze(&self, resolver: &R) -> Result<Self::Frozen, R::Error> {
    Ok(match self {
      Self::Primitive(primitive) => FrozenTy::Primitive(*primitive),
      Self::UnfrozenScalar(scalar) => FrozenTy::FrozenScalar(scalar.freeze(resolver).await?),
      Self::UnfrozenArray(array) => FrozenTy::FrozenArray(array.freeze(resolver).await?),
      Self::UnfrozenOption(option) => FrozenTy::FrozenOption(option.freeze(resolver).await?),
    })
  }
}

impl UnfrozenTy {
  /// Collect the record references this type expression depends on.
  pub fn dependencies<'a>(&'a self, set: &mut std::collections::HashSet<&'a UnfrozenReference>) {
    match self {
      Self::Primitive(_) => {}
      Self::UnfrozenScalar(scalar) => {
        set.insert(&scalar.reference);
      }
      Self::UnfrozenArray(array) => {
        set.insert(&array.reference);
      }
      Self::UnfrozenOption(option) => option.element.dependencies(set),
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::record::Version;
  use uuid::Uuid;

  fn reference() -> FrozenReference {
    FrozenReference {
      id: Uuid::from_u128(0x0bf1),
      version: Version::parse("1.2.0").expect("a version"),
    }
  }

  /// An optional's element carries its dependencies, and the form survives the
  /// record's serialized shape.
  #[test]
  fn an_optional_depends_on_its_element_and_round_trips() {
    let optional = FrozenTy::FrozenOption(FrozenOption {
      element: Box::new(FrozenTy::FrozenScalar(FrozenScalar {
        reference: reference(),
      })),
    });
    let mut dependencies = std::collections::HashSet::new();
    optional.dependencies(&mut dependencies);
    assert_eq!(
      dependencies.into_iter().collect::<Vec<_>>(),
      vec![&reference()]
    );

    let json = serde_json::to_string(&optional).expect("serializes");
    assert_eq!(
      serde_json::from_str::<FrozenTy>(&json).expect("deserializes"),
      optional
    );
    let primitive = FrozenTy::FrozenOption(FrozenOption {
      element: Box::new(FrozenTy::from(PrimitiveKind::F32)),
    });
    assert!(primitive.is_option());
    let mut none = std::collections::HashSet::new();
    primitive.dependencies(&mut none);
    assert!(
      none.is_empty(),
      "a primitive element has no record dependency"
    );
  }

  /// A header's type reference freezes as a module record declares it: a
  /// primitive (or an array or optional of one) as that primitive, anything
  /// else as a reference pinned at the version given.
  #[test]
  fn a_type_ref_freezes_as_a_record_declares_it() {
    use crate::module::low::TypeRef;
    use crate::ty;
    let version = Version::parse("1.2.0").expect("a version");
    let record = Uuid::from_u128(0x0bf1);
    let pinned = |id: Uuid| FrozenReference {
      id,
      version: version.clone(),
    };
    let freeze = |type_ref: TypeRef| FrozenTy::from_type_ref(&type_ref, &version);

    assert_eq!(
      freeze(TypeRef::Scalar { id: *ty::F32_ID }),
      Some(FrozenTy::from(PrimitiveKind::F32))
    );
    assert_eq!(
      freeze(TypeRef::Array { id: *ty::STRING_ID }),
      Some(FrozenTy::from(PrimitiveKind::ArrayString))
    );
    assert_eq!(
      freeze(TypeRef::Option { id: *ty::U64_ID }),
      Some(FrozenTy::FrozenOption(FrozenOption {
        element: Box::new(FrozenTy::from(PrimitiveKind::U64)),
      }))
    );
    assert_eq!(
      freeze(TypeRef::Scalar { id: record }),
      Some(FrozenTy::FrozenScalar(FrozenScalar {
        reference: pinned(record)
      }))
    );
    assert_eq!(
      freeze(TypeRef::Array { id: record }),
      Some(FrozenTy::FrozenArray(FrozenArray {
        reference: pinned(record)
      }))
    );
    assert_eq!(
      freeze(TypeRef::Option { id: record }),
      Some(FrozenTy::FrozenOption(FrozenOption {
        element: Box::new(FrozenTy::FrozenScalar(FrozenScalar {
          reference: pinned(record)
        })),
      }))
    );
    // A well-known type that is not a primitive is named by reference.
    assert_eq!(
      freeze(TypeRef::Scalar {
        id: *ty::KEY_VALUE_ID
      }),
      Some(FrozenTy::FrozenScalar(FrozenScalar {
        reference: pinned(*ty::KEY_VALUE_ID)
      }))
    );
    // Neither a fixed-length array nor a map has a record form.
    assert_eq!(
      freeze(TypeRef::FixedArray {
        id: *ty::F64_ID,
        len: 3
      }),
      None
    );
    assert_eq!(
      freeze(TypeRef::Map {
        key_id: *ty::STRING_ID,
        value_id: *ty::U8_ID
      }),
      None
    );
  }

  #[test]
  fn a_primitive_is_named_by_its_well_known_id() {
    use crate::ty;
    assert_eq!(
      PrimitiveKind::from_id(&ty::BOOLEAN_ID),
      Some(PrimitiveKind::Boolean)
    );
    assert_eq!(
      PrimitiveKind::from_id(&ty::ARRAY_F64_ID),
      Some(PrimitiveKind::ArrayF64)
    );
    assert_eq!(PrimitiveKind::from_id(&ty::UUID_ID), None);
    assert_eq!(PrimitiveKind::from_id(&Uuid::from_u128(0x0bf1)), None);
    assert_eq!(PrimitiveKind::Unit.array_of(), None);
    assert_eq!(PrimitiveKind::ArrayU8.array_of(), None);
    assert_eq!(PrimitiveKind::U8.array_of(), Some(PrimitiveKind::ArrayU8));
  }
}
