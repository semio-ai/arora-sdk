//! The `enumeration` type record: a named sum type whose variants are typed by
//! [`super::ty`] expressions. Same unfrozen/frozen split as
//! [`super::structure`].

use async_trait::async_trait;

use super::freeze::{Freeze, Resolver};

/// Builder (unfrozen) forms: what a type-spec factory produces.
pub mod unfrozen {
  use super::super::ty::UnfrozenTy;
  use indexmap::IndexMap;
  use serde::{Deserialize, Serialize};
  use uuid::Uuid;

  #[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
  #[serde(rename = "enumeration_V0_Variant", rename_all = "camelCase")]
  pub struct EnumerationVariant {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: UnfrozenTy,
  }

  #[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
  #[serde(rename = "enumeration_V0_Private", rename_all = "camelCase")]
  pub struct Enumeration {
    pub parent: Uuid,
    pub name: String,
    /// `IndexMap` so variant order is preserved through serialization.
    pub variants: IndexMap<Uuid, EnumerationVariant>,
  }
}

/// Frozen (version-pinned) forms — the record wire format.
pub mod frozen {
  use super::super::ty::FrozenTy;
  use indexmap::IndexMap;
  use serde::{Deserialize, Serialize};
  use uuid::Uuid;

  #[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
  #[serde(rename = "enumeration_V0_Frozen_Variant", rename_all = "camelCase")]
  pub struct EnumerationVariant {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: FrozenTy,
  }

  #[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
  #[serde(rename = "enumeration_V0_Frozen", rename_all = "camelCase")]
  pub struct Enumeration {
    pub parent: Uuid,
    pub name: String,
    /// `IndexMap` so variant order is preserved through serialization.
    pub variants: IndexMap<Uuid, EnumerationVariant>,
  }

  impl Enumeration {
    pub fn variant_named(&self, name: &str) -> Option<&EnumerationVariant> {
      self.variants.values().find(|variant| variant.name == name)
    }
  }
}

#[async_trait]
impl<R: Resolver> Freeze<R> for unfrozen::EnumerationVariant {
  type Frozen = frozen::EnumerationVariant;

  async fn freeze(&self, resolver: &R) -> Result<Self::Frozen, R::Error> {
    Ok(frozen::EnumerationVariant {
      name: self.name.clone(),
      ty: self.ty.freeze(resolver).await?,
    })
  }
}

#[async_trait]
impl<R: Resolver> Freeze<R> for unfrozen::Enumeration {
  type Frozen = frozen::Enumeration;

  async fn freeze(&self, resolver: &R) -> Result<Self::Frozen, R::Error> {
    let mut variants = indexmap::IndexMap::with_capacity(self.variants.len());
    for (id, variant) in &self.variants {
      variants.insert(*id, variant.freeze(resolver).await?);
    }
    Ok(frozen::Enumeration {
      parent: self.parent,
      name: self.name.clone(),
      variants,
    })
  }
}
