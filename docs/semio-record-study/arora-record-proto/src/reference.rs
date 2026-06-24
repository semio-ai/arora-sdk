//! Reference + version newtypes.
//!
//! These mirror semio-record's `record::{Version, VersionReq, FrozenReference,
//! UnfrozenReference}` (semio-record/crates/semio-record/src/record.rs:25-98)
//! but live in a neutral, store-agnostic crate.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A concrete, pinned version. Newtype over `semver::Version`, exactly like
/// semio-record `record::Version` (record.rs:27).
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

/// A version requirement (range). `None` means "any". Mirrors semio-record
/// `record::VersionReq` (record.rs:43).
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

impl std::fmt::Display for VersionReq {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match &self.0 {
      Some(req) => write!(f, "{}", req),
      None => write!(f, "*"),
    }
  }
}

/// Unpinned reference: an id plus a version *requirement*. Mirrors semio-record
/// `record::UnfrozenReference` (record.rs:85).
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

/// Pinned reference: an id plus a *concrete* version. Mirrors semio-record
/// `record::FrozenReference` (record.rs:77).
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
