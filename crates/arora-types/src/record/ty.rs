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
use super::reference::{FrozenReference, UnfrozenReference};

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
}
