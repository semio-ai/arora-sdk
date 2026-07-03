//! Type records: versioned type specifications and the machinery to pin them.
//!
//! This module is the home of arora's "record" vocabulary — deliberately *in*
//! `arora-types` rather than a separate crate, because declaring a type and
//! pinning which version of it you mean are one workflow: arora-types offers
//! **factories that produce structures from type specifications that can be
//! versioned**. A [`structure`], [`enumeration`] or [`module`] record is
//! declared in its *unfrozen* form (references carry version requirements),
//! then a [`freeze::Resolver`] — a registry — pins every reference to a
//! concrete [`reference::Version`], yielding the *frozen* form that goes on
//! the wire. Splitting the specs from the versioning machinery would cut the
//! factories from their inputs.
//!
//! The frozen serde shapes are wire-compatible with the Semio store's record
//! format (semio-record), which this module replaces for arora's needs.
//! Registries provide a [`freeze::Resolver`]; any type implements
//! [`freeze::Freeze`] to be pinned, and [`versioned::Versioned`] to carry
//! identity + version.

pub mod enumeration;
pub mod folder;
pub mod freeze;
pub mod module;
pub mod reference;
pub mod selector;
pub mod structure;
pub mod ty;
pub mod versioned;

pub use freeze::{Freeze, Resolver};
pub use reference::{FrozenReference, UnfrozenReference, Version, VersionReq};
pub use selector::{RecordType, Selector};
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

#[cfg(test)]
mod wire_tests;
