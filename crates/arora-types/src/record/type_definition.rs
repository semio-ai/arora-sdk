//! A type record's frozen definition: what a registry answers for a type id at
//! a version, and what `#[derive(AroraType)]` produces for a Rust type.

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::enumeration::frozen::Enumeration;
use super::folder::{public::Public as FolderPublic, DeclaredFolder};
use super::structure::frozen::Structure;
use super::ty::{FrozenTy, PrimitiveKind};
use super::FrozenReference;

/// A frozen type definition: a primitive, which has no record of its own, or
/// a structure or an enumeration record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TypeDefinitionFrozen {
  Primitive(PrimitiveKind),
  Enumeration(Enumeration),
  Structure(Structure),
}

impl TypeDefinitionFrozen {
  /// The type's name: a primitive's (`f32`, `str`), or the record's.
  pub fn name(&self) -> String {
    match self {
      Self::Primitive(primitive) => primitive.to_string(),
      Self::Enumeration(enumeration, ..) => enumeration.name.to_owned(),
      Self::Structure(structure, ..) => structure.name.to_owned(),
    }
  }

  /// The ids of the records its fields or variants name.
  pub fn direct_dependencies(&self) -> HashSet<Uuid> {
    self
      .references()
      .into_iter()
      .map(|reference| reference.id)
      .collect()
  }

  /// The pinned references its fields or variants name: the records a
  /// registry holding this one must hold too.
  pub fn references(&self) -> HashSet<super::FrozenReference> {
    let mut references = HashSet::new();
    let mut insert = |ty: &FrozenTy| {
      let mut found = HashSet::new();
      ty.dependencies(&mut found);
      references.extend(found.into_iter().cloned());
    };
    match self {
      Self::Primitive(_) => {}
      Self::Enumeration(enumeration) => enumeration
        .variants
        .values()
        .for_each(|variant| insert(&variant.ty)),
      Self::Structure(structure) => structure
        .fields
        .values()
        .for_each(|field| insert(&field.ty)),
    }
    references
  }
}

/// The records describing a set of types, as a registry holds them: each
/// type's structure or enumeration record under the reference that pins it,
/// and the folders the types declare, with the folders holding those. What
/// [`AroraType::register_records`](crate::AroraType::register_records) and
/// [`AroraModule::type_records`](crate::module::declared::AroraModule::type_records)
/// produce.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TypeRecords {
  /// The folders, by id.
  pub folders: BTreeMap<Uuid, FolderPublic>,
  /// The type records, by the reference that pins each.
  pub types: BTreeMap<FrozenReference, TypeDefinitionFrozen>,
}

impl TypeRecords {
  /// Hold `folder` and the folders holding it.
  pub fn insert_folder(&mut self, folder: &DeclaredFolder) {
    self.folders.extend(folder.records());
  }
}
