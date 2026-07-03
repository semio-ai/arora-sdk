//! Reference + version newtypes for the neutral record model.
//!
//! These mirror semio-record's `record::{Version, VersionReq, FrozenReference,
//! UnfrozenReference}` but live here in the neutral, store-agnostic interface
//! layer so arora can express "versioning + freezing" without depending on the
//! private `semio-record` crate. See `docs/semio-record-study/STUDY.md` in
//! arora-engine for the migration this is the foundation of.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A concrete, pinned version. Newtype over `semver::Version`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Version(pub semver::Version);

impl Version {
  pub fn parse(s: &str) -> Result<Self, semver::Error> {
    Ok(Version(s.parse()?))
  }
}

impl std::fmt::Display for Version {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "{}", self.0)
  }
}

impl From<semver::Version> for Version {
  fn from(v: semver::Version) -> Self {
    Version(v)
  }
}

impl From<crate::SemanticVersion> for Version {
  fn from(v: crate::SemanticVersion) -> Self {
    Version(v.into())
  }
}

/// A version requirement (range). `None` means "any".
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct VersionReq(pub Option<semver::VersionReq>);

impl VersionReq {
  pub fn any() -> Self {
    VersionReq(None)
  }
  pub fn parse(s: &str) -> Result<Self, semver::Error> {
    Ok(VersionReq(Some(s.parse()?)))
  }
  /// Does a concrete version satisfy this requirement?
  pub fn matches(&self, version: &Version) -> bool {
    match &self.0 {
      Some(req) => req.matches(&version.0),
      None => true,
    }
  }
}

impl Default for VersionReq {
  fn default() -> Self {
    Self::any()
  }
}

impl std::fmt::Display for VersionReq {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match &self.0 {
      Some(req) => write!(f, "{}", req),
      None => write!(f, "*"),
    }
  }
}

/// Unpinned reference: an id plus a version *requirement*.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnfrozenReference {
  pub id: Uuid,
  pub version_req: VersionReq,
}

impl std::fmt::Display for UnfrozenReference {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match &self.version_req.0 {
      Some(req) => write!(f, "{}@{}", self.id, req),
      None => write!(f, "{}", self.id),
    }
  }
}

/// Pinned reference: an id plus a *concrete* version.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrozenReference {
  pub id: Uuid,
  pub version: Version,
}

impl std::fmt::Display for FrozenReference {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "{}@{}", self.id, self.version)
  }
}
