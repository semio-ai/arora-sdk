//! Tests for the neutral record model.
//!
//! The trait machinery is exercised against example types (mirroring real
//! arora shapes) plus an `InMemoryRegistry` resolver, then the real
//! `Versioned for Header` integration. Ported from the #87 study prototype
//! (`docs/semio-record-study/arora-record-proto`, 8 tests, all green).

use super::{
  Compat, Freeze, FrozenReference, Resolver, UnfrozenReference, Version, VersionReq, Versioned,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Example types mirroring real arora shapes (ty::low::TypeRef / Structure /
// module::low::Header), used only to exercise the traits.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PrimitiveKind {
  Unit,
  Boolean,
  I32,
  F64,
  String,
}

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
      fields: self.fields.freeze(resolver)?, // blanket HashMap impl
    })
  }
}

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

// ---------------------------------------------------------------------------
// An in-memory registry implementing `Resolver`, like LocalRegistry::freeze.
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
pub struct InMemoryRegistry {
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
      ResolveError::NoSuchVersion(id, req) => write!(f, "no version of {} satisfies {}", id, req),
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
    // Newest version matching the requirement — same policy as
    // arora-registry LocalRegistry::freeze (`.rev().find`).
    let version = versions
      .iter()
      .rev()
      .find(|v| reference.version_req.matches(v))
      .cloned()
      .ok_or_else(|| ResolveError::NoSuchVersion(reference.id, reference.version_req.clone()))?;
    Ok(FrozenReference {
      id: reference.id,
      version,
    })
  }
}

// ---------------------------------------------------------------------------
// Unfreeze: widen a frozen reference back to an exact-version requirement.
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

fn v(s: &str) -> Version {
  Version::parse(s).unwrap()
}

#[test]
fn version_compat_rules() {
  assert_eq!(
    Structure::compatibility(&v("1.2.0"), &v("1.2.0")),
    Compat::Identical
  );
  assert_eq!(
    Structure::compatibility(&v("1.2.0"), &v("1.5.0")),
    Compat::BackwardCompatible
  );
  assert_eq!(
    Structure::compatibility(&v("1.2.0"), &v("2.0.0")),
    Compat::Incompatible
  );
  assert_eq!(
    Structure::compatibility(&v("1.5.0"), &v("1.2.0")),
    Compat::Incompatible
  );
}

#[test]
fn versioned_reference_tagging() {
  let id = Uuid::new_v4();
  let s = Structure {
    id,
    version: v("3.1.4"),
    name: "Pose".into(),
    fields: HashMap::new(),
  };
  assert_eq!(s.frozen_reference().version, v("3.1.4"));
  assert_eq!(s.frozen_reference().id, id);

  let unfrozen = s.unfrozen_reference(VersionReq::parse(">=3.0.0").unwrap());
  assert_eq!(unfrozen.id, id);
  assert!(unfrozen.version_req.matches(&v("3.2.0")));
  assert!(!unfrozen.version_req.matches(&v("2.9.0")));
}

#[test]
fn resolver_picks_newest_matching() {
  let id = Uuid::new_v4();
  let mut reg = InMemoryRegistry::new();
  reg.publish(id, v("1.0.0"));
  reg.publish(id, v("1.4.0"));
  reg.publish(id, v("2.0.0"));

  let any = UnfrozenTy::Scalar(UnfrozenReference {
    id,
    version_req: VersionReq::any(),
  });
  match any.freeze(&reg).unwrap() {
    FrozenTy::Scalar(r) => assert_eq!(r.version, v("2.0.0")),
    other => panic!("expected scalar, got {:?}", other),
  }

  let capped = UnfrozenTy::Scalar(UnfrozenReference {
    id,
    version_req: VersionReq::parse("<2.0.0").unwrap(),
  });
  match capped.freeze(&reg).unwrap() {
    FrozenTy::Scalar(r) => assert_eq!(r.version, v("1.4.0")),
    other => panic!("expected scalar, got {:?}", other),
  }
}

#[test]
fn resolver_errors_surface() {
  let mut reg = InMemoryRegistry::new();
  let id = Uuid::new_v4();
  reg.publish(id, v("1.0.0"));

  let missing = UnfrozenReference {
    id: Uuid::new_v4(),
    version_req: VersionReq::any(),
  };
  assert!(matches!(
    reg.resolve(&missing).unwrap_err(),
    ResolveError::NoSuchRecord(_)
  ));

  let bad_req = UnfrozenReference {
    id,
    version_req: VersionReq::parse(">=2.0.0").unwrap(),
  };
  assert!(matches!(
    reg.resolve(&bad_req).unwrap_err(),
    ResolveError::NoSuchVersion(_, _)
  ));
}

#[test]
fn freeze_unfreeze_roundtrip_pins_exact_version() {
  let id = Uuid::new_v4();
  let mut reg = InMemoryRegistry::new();
  reg.publish(id, v("1.0.0"));
  reg.publish(id, v("1.7.2"));

  let original = UnfrozenTy::Array(UnfrozenReference {
    id,
    version_req: VersionReq::parse("^1.0").unwrap(),
  });

  let frozen = original.freeze(&reg).unwrap();
  let widened = frozen.unfreeze();
  let refrozen = widened.freeze(&reg).unwrap();
  assert_eq!(frozen, refrozen);

  match widened {
    UnfrozenTy::Array(r) => {
      assert!(r.version_req.matches(&v("1.7.2")) && !r.version_req.matches(&v("1.0.0")))
    }
    other => panic!("expected array, got {:?}", other),
  }
}

#[test]
fn freeze_structure_record() {
  let dep_id = Uuid::new_v4();
  let mut reg = InMemoryRegistry::new();
  reg.publish(dep_id, v("0.9.0"));
  reg.publish(dep_id, v("1.2.0"));

  let mut fields = HashMap::new();
  fields.insert(
    Uuid::new_v4(),
    StructureField {
      name: "count".into(),
      ty: UnfrozenTy::Primitive(PrimitiveKind::I32),
    },
  );
  fields.insert(
    Uuid::new_v4(),
    StructureField {
      name: "child".into(),
      ty: UnfrozenTy::Scalar(UnfrozenReference {
        id: dep_id,
        version_req: VersionReq::parse("^1.0").unwrap(),
      }),
    },
  );

  let s = Structure {
    id: Uuid::new_v4(),
    version: v("1.0.0"),
    name: "Robot".into(),
    fields,
  };

  let frozen = s.freeze(&reg).unwrap();
  let child = frozen.fields.values().find(|f| f.name == "child").unwrap();
  match &child.ty {
    FrozenTy::Scalar(r) => assert_eq!(r.version, v("1.2.0")),
    other => panic!("expected scalar, got {:?}", other),
  }
}

#[test]
fn freeze_module_header_record() {
  let ty_id = Uuid::new_v4();
  let dep_id = Uuid::new_v4();
  let mut reg = InMemoryRegistry::new();
  reg.publish(ty_id, v("2.3.0"));
  reg.publish(dep_id, v("0.1.0"));
  reg.publish(dep_id, v("0.2.0"));

  let header = ModuleHeader {
    id: Uuid::new_v4(),
    version: v("1.0.0"),
    name: "polly".into(),
    parameters: vec![Parameter {
      name: "input".into(),
      ty: UnfrozenTy::Scalar(UnfrozenReference {
        id: ty_id,
        version_req: VersionReq::any(),
      }),
    }],
    dependencies: vec![UnfrozenReference {
      id: dep_id,
      version_req: VersionReq::parse(">=0.1,<0.2").unwrap(),
    }],
  };

  let frozen = header.freeze(&reg).unwrap();
  assert_eq!(frozen.dependencies[0].version, v("0.1.0"));
  match &frozen.parameters[0].ty {
    FrozenTy::Scalar(r) => assert_eq!(r.version, v("2.3.0")),
    other => panic!("expected scalar, got {:?}", other),
  }
}

#[test]
fn frozen_form_serde_roundtrips() {
  let frozen = FrozenTy::Scalar(FrozenReference {
    id: Uuid::nil(),
    version: v("1.2.3"),
  });
  let json = serde_json::to_string(&frozen).unwrap();
  let back: FrozenTy = serde_json::from_str(&json).unwrap();
  assert_eq!(frozen, back);
  assert!(json.contains("\"kind\":\"scalar\""));
}

/// The real arora-types `Header` is `Versioned` (Phase 1 integration).
#[test]
fn header_is_versioned() {
  use crate::module::low::{Executor, Header};
  use crate::SemanticVersion;

  let id = Uuid::new_v4();
  let header = Header {
    id,
    name: "polly".into(),
    author: "Semio".into(),
    description: None,
    license: "Proprietary".into(),
    version: SemanticVersion {
      major: 1,
      minor: 4,
      patch: 2,
    },
    executor: Executor {
      name: "wasm".into(),
      min_version: None,
      max_version: None,
    },
    exports: vec![],
    imports: vec![],
    executable_mime: "application/wasm".into(),
  };

  assert_eq!(header.id(), id);
  assert_eq!(header.version(), v("1.4.2"));
  assert_eq!(header.frozen_reference().version, v("1.4.2"));
}
