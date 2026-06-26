//! `Versioned` — version tagging + compatibility, applicable to any type.
//!
//! semio-record bakes identity/version into `RecordDefn` (const TYPE +
//! SCHEMA_VERSION) and into the `*Reference` structs. Here we split that into a
//! reusable trait so an arbitrary type (a module header, a structure, an
//! enumeration, a behavior-tree type, ...) can declare "I have an identity and
//! a version, and here is how two versions of me relate".

use crate::record::reference::{FrozenReference, UnfrozenReference, Version, VersionReq};
use uuid::Uuid;

/// Result of comparing two versions of the same logical thing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compat {
  /// Same version.
  Identical,
  /// Newer is a backward-compatible superset (semver: same major, >= minor).
  BackwardCompatible,
  /// Versions are incompatible (major bump, or older-than).
  Incompatible,
}

/// A type that carries a stable identity and a semantic version, and knows how
/// to express references to itself and compatibility between its versions.
pub trait Versioned {
  fn id(&self) -> Uuid;
  fn version(&self) -> Version;

  /// A pinned reference to this exact value.
  fn frozen_reference(&self) -> FrozenReference {
    FrozenReference {
      id: self.id(),
      version: self.version(),
    }
  }

  /// An unpinned reference requesting any version satisfying `req`.
  fn unfrozen_reference(&self, req: VersionReq) -> UnfrozenReference {
    UnfrozenReference {
      id: self.id(),
      version_req: req,
    }
  }

  /// Default semver compatibility policy. Override per-type if a record kind
  /// has special rules (e.g. schema migrations).
  fn compatibility(from: &Version, to: &Version) -> Compat {
    if from == to {
      return Compat::Identical;
    }
    if to.0.major == from.0.major && to.0 >= from.0 {
      Compat::BackwardCompatible
    } else {
      Compat::Incompatible
    }
  }
}
