//! Realistic example types showing the traits applied to >= 2 distinct types,
//! modeled on the real arora-types / semio-record shapes.
//!
//! Type 1: `UnfrozenTy` / `FrozenTy` — mirrors semio-record `ty::UnfrozenTy`
//!         (ty.rs:424) and arora-types `module::low::TypeRef` (module/low.rs:17).
//! Type 2: `Structure` — mirrors semio-record `structure::v0::unfrozen::Structure`
//!         and arora-types `ty::low::Structure` (ty/low.rs:16).
//! Plus an in-memory `Registry` implementing `Resolver` + a round-trip unfreeze.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::freeze::{Freeze, Resolver};
use crate::reference::{FrozenReference, UnfrozenReference, Version, VersionReq};
use crate::versioned::Versioned;

// ----------------------------------------------------------------------------
// Primitive kinds (shared between frozen and unfrozen), like ty::PrimitiveKind.
// ----------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PrimitiveKind {
  Unit,
  Boolean,
  I32,
  F64,
  String,
}

// ----------------------------------------------------------------------------
// Type 1: a type reference, in unfrozen and frozen forms.
// ----------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub enum UnfrozenTy {
  Primitive(PrimitiveKind),
  Scalar(UnfrozenReference),
  Array(UnfrozenReference),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub enum FrozenTy {
  Primitive(PrimitiveKind),
  Scalar(FrozenReference),
  Array(FrozenReference),
}

impl<R: Resolver> Freeze<R> for UnfrozenTy {
  type Frozen = FrozenTy;
  fn freeze(&self, resolver: &R) -> Result<Self::Frozen, R::Error> {
    Ok(match self {
      UnfrozenTy::Primitive(p) => FrozenTy::Primitive(p.clone()),
      UnfrozenTy::Scalar(r) => FrozenTy::Scalar(r.freeze(resolver)?),
      UnfrozenTy::Array(r) => FrozenTy::Array(r.freeze(resolver)?),
    })
  }
}

// ----------------------------------------------------------------------------
// Type 2: a Structure record (fields referencing other types).
// ----------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructureField {
  pub name: String,
  #[serde(rename = "type")]
  pub ty: UnfrozenTy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrozenStructureField {
  pub name: String,
  #[serde(rename = "type")]
  pub ty: FrozenTy,
}

impl<R: Resolver> Freeze<R> for StructureField {
  type Frozen = FrozenStructureField;
  fn freeze(&self, resolver: &R) -> Result<Self::Frozen, R::Error> {
    Ok(FrozenStructureField {
      name: self.name.clone(),
      ty: self.ty.freeze(resolver)?,
    })
  }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Structure {
  pub id: Uuid,
  pub version: Version,
  pub name: String,
  pub fields: HashMap<Uuid, StructureField>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrozenStructure {
  pub id: Uuid,
  pub version: Version,
  pub name: String,
  pub fields: HashMap<Uuid, FrozenStructureField>,
}

impl Versioned for Structure {
  fn id(&self) -> Uuid {
    self.id
  }
  fn version(&self) -> Version {
    self.version.clone()
  }
}

impl<R: Resolver> Freeze<R> for Structure {
  type Frozen = FrozenStructure;
  fn freeze(&self, resolver: &R) -> Result<Self::Frozen, R::Error> {
    Ok(FrozenStructure {
      id: self.id,
      version: self.version.clone(),
      name: self.name.clone(),
      // Uses the blanket HashMap<_, Freeze> impl.
      fields: self.fields.freeze(resolver)?,
    })
  }
}

// ----------------------------------------------------------------------------
// A second distinct record type: a module header, to prove the trait is not
// hard-wired to one shape. Mirrors arora-types module::low::Header.
// ----------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Parameter {
  pub name: String,
  #[serde(rename = "type")]
  pub ty: UnfrozenTy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrozenParameter {
  pub name: String,
  #[serde(rename = "type")]
  pub ty: FrozenTy,
}

impl<R: Resolver> Freeze<R> for Parameter {
  type Frozen = FrozenParameter;
  fn freeze(&self, resolver: &R) -> Result<Self::Frozen, R::Error> {
    Ok(FrozenParameter {
      name: self.name.clone(),
      ty: self.ty.freeze(resolver)?,
    })
  }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModuleHeader {
  pub id: Uuid,
  pub version: Version,
  pub name: String,
  pub parameters: Vec<Parameter>,
  pub dependencies: Vec<UnfrozenReference>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrozenModuleHeader {
  pub id: Uuid,
  pub version: Version,
  pub name: String,
  pub parameters: Vec<FrozenParameter>,
  pub dependencies: Vec<FrozenReference>,
}

impl Versioned for ModuleHeader {
  fn id(&self) -> Uuid {
    self.id
  }
  fn version(&self) -> Version {
    self.version.clone()
  }
}

impl<R: Resolver> Freeze<R> for ModuleHeader {
  type Frozen = FrozenModuleHeader;
  fn freeze(&self, resolver: &R) -> Result<Self::Frozen, R::Error> {
    Ok(FrozenModuleHeader {
      id: self.id,
      version: self.version.clone(),
      name: self.name.clone(),
      parameters: self.parameters.freeze(resolver)?, // blanket Vec impl
      dependencies: self.dependencies.freeze(resolver)?, // blanket Vec impl
    })
  }
}

// ----------------------------------------------------------------------------
// An in-memory registry implementing `Resolver`, like LocalRegistry::freeze.
// ----------------------------------------------------------------------------

#[derive(Debug, Default)]
pub struct InMemoryRegistry {
  /// id -> sorted available versions.
  versions: HashMap<Uuid, Vec<Version>>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ResolveError {
  NoSuchRecord(Uuid),
  NoSuchVersion(Uuid, VersionReq),
}

impl std::fmt::Display for ResolveError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      ResolveError::NoSuchRecord(id) => write!(f, "no such record: {}", id),
      ResolveError::NoSuchVersion(id, req) => {
        write!(f, "no version of {} satisfies {}", id, req)
      }
    }
  }
}

impl std::error::Error for ResolveError {}

impl InMemoryRegistry {
  pub fn new() -> Self {
    Self::default()
  }

  pub fn publish(&mut self, id: Uuid, version: Version) {
    let entry = self.versions.entry(id).or_default();
    entry.push(version);
    entry.sort();
  }
}

impl Resolver for InMemoryRegistry {
  type Error = ResolveError;

  fn resolve(&self, reference: &UnfrozenReference) -> Result<FrozenReference, Self::Error> {
    let versions = self
      .versions
      .get(&reference.id)
      .ok_or(ResolveError::NoSuchRecord(reference.id))?;
    // Pick the highest version matching the requirement — same policy as
    // arora-registry LocalRegistry::freeze (local/mod.rs:117 `.rev().find`).
    let version = versions
      .iter()
      .rev()
      .find(|v| reference.version_req.matches(v))
      .cloned()
      .ok_or_else(|| {
        ResolveError::NoSuchVersion(reference.id, reference.version_req.clone())
      })?;
    Ok(FrozenReference {
      id: reference.id,
      version,
    })
  }
}

// ----------------------------------------------------------------------------
// Unfreeze: widen a frozen reference back to an unfrozen one. Demonstrates the
// round trip. Freezing is lossy on the *requirement* (we forget the original
// range), so unfreezing reconstructs an exact-version requirement (`=x.y.z`),
// which is the natural inverse: "I want exactly the version I was pinned to".
// ----------------------------------------------------------------------------

pub trait Unfreeze {
  type Unfrozen;
  fn unfreeze(&self) -> Self::Unfrozen;
}

impl Unfreeze for FrozenReference {
  type Unfrozen = UnfrozenReference;
  fn unfreeze(&self) -> Self::Unfrozen {
    let exact = semver::VersionReq::parse(&format!("={}", self.version.0))
      .expect("exact version is a valid requirement");
    UnfrozenReference {
      id: self.id,
      version_req: VersionReq(Some(exact)),
    }
  }
}

impl Unfreeze for FrozenTy {
  type Unfrozen = UnfrozenTy;
  fn unfreeze(&self) -> Self::Unfrozen {
    match self {
      FrozenTy::Primitive(p) => UnfrozenTy::Primitive(p.clone()),
      FrozenTy::Scalar(r) => UnfrozenTy::Scalar(r.unfreeze()),
      FrozenTy::Array(r) => UnfrozenTy::Array(r.unfreeze()),
    }
  }
}
