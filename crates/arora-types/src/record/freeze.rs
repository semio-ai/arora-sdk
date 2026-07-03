//! `Freeze` + `Resolver` — the generalized freezing mechanism.
//!
//! semio-record defines an async `Freeze<F: Resolver>` + `Resolver`. Generalized
//! here, store-agnostic:
//!   * `Resolver` plays the role of `Resolver`. The real, store-backed
//!     implementation (arora-registry) implements this trait — and because the
//!     remote registry resolves over the network, this is **async**, matching
//!     semio-record's `#[async_trait] Resolver`.
//!   * `Freeze<R>` is identical in spirit but NOT tied to module records.
//!   * Blanket impls give "freezing a container freezes its elements" for free,
//!     so most leaf types are the only ones that need a hand-written impl.

use crate::record::reference::{FrozenReference, UnfrozenReference};
use async_trait::async_trait;
use std::collections::HashMap;
use std::hash::Hash;

/// Resolves an unpinned reference into a pinned one by choosing a concrete
/// version. The ONLY abstraction that needs to touch a store/registry. Mirrors
/// semio-record `record::Resolver` and arora-registry's `impl Resolver` (which
/// picks the newest version matching the requirement).
#[async_trait]
pub trait Resolver: Sync {
  type Error: std::error::Error + Send;
  async fn resolve(&self, reference: &UnfrozenReference) -> Result<FrozenReference, Self::Error>;
}

/// A value that can be frozen by resolver `R`. `Frozen` is the pinned form.
#[async_trait]
pub trait Freeze<R: Resolver> {
  type Frozen;
  async fn freeze(&self, resolver: &R) -> Result<Self::Frozen, R::Error>;
}

// --- Blanket impls: structure-preserving freezing of common containers. ---

#[async_trait]
impl<R, T> Freeze<R> for Vec<T>
where
  R: Resolver,
  T: Freeze<R> + Sync,
  T::Frozen: Send,
{
  type Frozen = Vec<T::Frozen>;
  async fn freeze(&self, resolver: &R) -> Result<Self::Frozen, R::Error> {
    let mut out = Vec::with_capacity(self.len());
    for item in self {
      out.push(item.freeze(resolver).await?);
    }
    Ok(out)
  }
}

#[async_trait]
impl<R, T> Freeze<R> for Option<T>
where
  R: Resolver,
  T: Freeze<R> + Sync,
  T::Frozen: Send,
{
  type Frozen = Option<T::Frozen>;
  async fn freeze(&self, resolver: &R) -> Result<Self::Frozen, R::Error> {
    Ok(match self {
      Some(value) => Some(value.freeze(resolver).await?),
      None => None,
    })
  }
}

#[async_trait]
impl<R, K, V> Freeze<R> for HashMap<K, V>
where
  R: Resolver,
  K: Clone + Eq + Hash + Send + Sync,
  V: Freeze<R> + Sync,
  V::Frozen: Send,
{
  type Frozen = HashMap<K, V::Frozen>;
  async fn freeze(&self, resolver: &R) -> Result<Self::Frozen, R::Error> {
    let mut out = HashMap::with_capacity(self.len());
    for (k, v) in self {
      out.insert(k.clone(), v.freeze(resolver).await?);
    }
    Ok(out)
  }
}

/// The base case: freezing a bare unfrozen reference asks the resolver to pin it.
#[async_trait]
impl<R: Resolver> Freeze<R> for UnfrozenReference {
  type Frozen = FrozenReference;
  async fn freeze(&self, resolver: &R) -> Result<Self::Frozen, R::Error> {
    resolver.resolve(self).await
  }
}
