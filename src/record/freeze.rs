//! `Freeze` + `Resolver` — the generalized freezing mechanism.
//!
//! semio-record defines an async `Freeze<F: Freezer>` + `Freezer`. Generalized
//! here, store-agnostic:
//!   * `Resolver` plays the role of `Freezer`. The real, store-backed
//!     implementation (arora-registry) implements this trait.
//!   * `Freeze<R>` is identical in spirit but NOT tied to module records.
//!   * Blanket impls give "freezing a container freezes its elements" for free,
//!     so most leaf types are the only ones that need a hand-written impl.
//!
//! Synchronous for now — faithful to the validated prototype and this lean
//! interface crate. When the async, store-backed registries implement
//! `Resolver`, an async sibling can be added without changing this surface.

use crate::record::reference::{FrozenReference, UnfrozenReference};
use std::collections::HashMap;
use std::hash::Hash;

/// Resolves an unpinned reference into a pinned one by choosing a concrete
/// version. The ONLY abstraction that needs to touch a store/registry. Mirrors
/// semio-record `record::Freezer` and arora-registry's `impl Freezer` (which
/// picks the newest version matching the requirement).
pub trait Resolver {
  type Error: std::error::Error;
  fn resolve(&self, reference: &UnfrozenReference) -> Result<FrozenReference, Self::Error>;
}

/// A value that can be frozen by resolver `R`. `Frozen` is the pinned form.
pub trait Freeze<R: Resolver> {
  type Frozen;
  fn freeze(&self, resolver: &R) -> Result<Self::Frozen, R::Error>;
}

// --- Blanket impls: structure-preserving freezing of common containers. ---

impl<R: Resolver, T: Freeze<R>> Freeze<R> for Vec<T> {
  type Frozen = Vec<T::Frozen>;
  fn freeze(&self, resolver: &R) -> Result<Self::Frozen, R::Error> {
    self.iter().map(|x| x.freeze(resolver)).collect()
  }
}

impl<R: Resolver, T: Freeze<R>> Freeze<R> for Option<T> {
  type Frozen = Option<T::Frozen>;
  fn freeze(&self, resolver: &R) -> Result<Self::Frozen, R::Error> {
    self.as_ref().map(|x| x.freeze(resolver)).transpose()
  }
}

impl<R: Resolver, K: Clone + Eq + Hash, V: Freeze<R>> Freeze<R> for HashMap<K, V> {
  type Frozen = HashMap<K, V::Frozen>;
  fn freeze(&self, resolver: &R) -> Result<Self::Frozen, R::Error> {
    self
      .iter()
      .map(|(k, v)| Ok((k.clone(), v.freeze(resolver)?)))
      .collect()
  }
}

/// The base case: freezing a bare unfrozen reference asks the resolver to pin it.
impl<R: Resolver> Freeze<R> for UnfrozenReference {
  type Frozen = FrozenReference;
  fn freeze(&self, resolver: &R) -> Result<Self::Frozen, R::Error> {
    resolver.resolve(self)
  }
}
