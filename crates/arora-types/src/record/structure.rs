//! The `structure` type record: a named product type whose fields are typed by
//! [`super::ty`] expressions. Unfrozen = declared with version *requirements*;
//! frozen = every reference pinned (the wire form the Semio store consumes).

use async_trait::async_trait;

use super::freeze::{Freeze, Resolver};

/// Builder (unfrozen) forms: what a type-spec factory produces.
pub mod unfrozen {
  use super::super::ty::UnfrozenTy;
  use indexmap::IndexMap;
  use serde::{Deserialize, Serialize};
  use uuid::Uuid;

  #[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
  #[serde(rename = "structure_V0_Field")]
  pub struct StructureField {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: UnfrozenTy,
  }

  #[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
  #[serde(rename = "structure_V0_Private")]
  pub struct Structure {
    pub parent: Uuid,
    pub name: String,
    /// `IndexMap` so field order is preserved through serialization.
    pub fields: IndexMap<Uuid, StructureField>,
  }
}

/// Frozen (version-pinned) forms — the record wire format.
pub mod frozen {
  use super::super::ty::FrozenTy;
  use indexmap::IndexMap;
  use serde::{Deserialize, Serialize};
  use uuid::Uuid;

  #[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
  #[serde(rename = "structure_V0_Frozen_Field")]
  pub struct StructureField {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: FrozenTy,
  }

  #[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
  #[serde(rename = "structure_V0_Frozen")]
  pub struct Structure {
    pub parent: Uuid,
    pub name: String,
    /// `IndexMap` so field order is preserved through serialization.
    pub fields: IndexMap<Uuid, StructureField>,
  }

  impl Structure {
    pub fn field_named(&self, name: &str) -> Option<&StructureField> {
      self.fields.values().find(|field| field.name == name)
    }
  }
}

#[async_trait]
impl<R: Resolver> Freeze<R> for unfrozen::StructureField {
  type Frozen = frozen::StructureField;

  async fn freeze(&self, resolver: &R) -> Result<Self::Frozen, R::Error> {
    Ok(frozen::StructureField {
      name: self.name.clone(),
      ty: self.ty.freeze(resolver).await?,
    })
  }
}

#[async_trait]
impl<R: Resolver> Freeze<R> for unfrozen::Structure {
  type Frozen = frozen::Structure;

  async fn freeze(&self, resolver: &R) -> Result<Self::Frozen, R::Error> {
    let mut fields = indexmap::IndexMap::with_capacity(self.fields.len());
    for (id, field) in &self.fields {
      fields.insert(*id, field.freeze(resolver).await?);
    }
    Ok(frozen::Structure {
      parent: self.parent,
      name: self.name.clone(),
      fields,
    })
  }
}
