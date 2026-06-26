//! Neutral "record" model: versioning + freezing, store-agnostic.
//!
//! This is the interface-layer generalization of semio-record's record/freeze
//! machinery, so arora crates can express version pinning ("freezing") without
//! depending on the private `semio-record` crate. Registries provide a
//! [`freeze::Resolver`]; any type implements [`freeze::Freeze`] to be pinned,
//! and [`versioned::Versioned`] to carry identity + version.

pub mod freeze;
pub mod reference;
pub mod versioned;

pub use freeze::{Freeze, Resolver};
pub use reference::{FrozenReference, UnfrozenReference, Version, VersionReq};
pub use versioned::{Compat, Versioned};

use crate::module::low::Header;
use uuid::Uuid;

/// A module header carries a stable id and a semantic version, so it is
/// naturally [`Versioned`]. (First real-type integration of the record model.)
impl Versioned for Header {
  fn id(&self) -> Uuid {
    self.id
  }
  fn version(&self) -> Version {
    Version(self.version.clone().into())
  }
}

#[cfg(test)]
mod tests;
