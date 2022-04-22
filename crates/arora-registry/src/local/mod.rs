mod editable;
mod readable;
mod reg_ref;
use self::reg_ref::{FrozenRegistryReference, LatestRegistryReference, LocalRegistryReference};
use crate::{
  EnumerationFrozen, EnumerationPublic, FolderPublic, ModuleFrozen, ModulePublic, RegistryError,
  StructureFrozen, StructurePublic,
};
use async_trait::async_trait;
use semio_client::common::Selector;
use semio_record::record::{Freezer, FrozenReference, UnfrozenReference};
use semver::Version;
use std::{
  collections::{btree_map, hash_map, BTreeMap, HashMap},
  rc::Rc,
};
use uuid::Uuid;

/// A [`LocalRegistry`] supports the addition of [`Structure`], [`Enumeration`] and [`Module`]
/// on the fly. It provides a local index to look them up fast
/// by [`Uuid'] or by path ([`String`]).
/// It can be used as a local cache of a remote registry accessed using [`semio_client`].
/// It provides an absolute root available for any record,
/// with the identifier [`ROOT_ID`].
pub struct LocalRegistry {
  latest_enumerations: Vec<Rc<EnumerationPublic>>,
  latest_structures: Vec<Rc<StructurePublic>>,
  latest_modules: Vec<Rc<ModulePublic>>,
  latest_folders: Vec<Rc<FolderPublic>>,
  latest_indexed: HashMap<Selector, LatestRegistryReference>,
  frozen_enumerations: Vec<Rc<EnumerationFrozen>>,
  frozen_structures: Vec<Rc<StructureFrozen>>,
  frozen_modules: Vec<Rc<ModuleFrozen>>,
  frozen_indexed: HashMap<Selector, BTreeMap<Version, FrozenRegistryReference>>,
  path_to_ids: HashMap<String, Uuid>,
}
unsafe impl Send for LocalRegistry {}
unsafe impl Sync for LocalRegistry {}

impl LocalRegistry {
  pub fn new() -> Self {
    Self {
      latest_enumerations: Vec::new(),
      latest_structures: Vec::new(),
      latest_modules: Vec::new(),
      latest_folders: Vec::new(),
      latest_indexed: HashMap::from([(
        Selector::Id(ROOT_ID.to_owned()),
        LatestRegistryReference::Root,
      )]),
      frozen_enumerations: Vec::new(),
      frozen_structures: Vec::new(),
      frozen_modules: Vec::new(),
      frozen_indexed: HashMap::from([(
        Selector::Id(ROOT_ID.to_owned()),
        BTreeMap::from([(Version::new(0, 0, 0), FrozenRegistryReference::Root)]),
      )]),
      path_to_ids: HashMap::new(),
    }
  }

  pub fn find(&self, selector: &Selector) -> Option<&LatestRegistryReference> {
    self.latest_indexed.get(selector)
  }

  pub fn find_id(&self, id: &Uuid) -> Option<&LatestRegistryReference> {
    self.find(&Selector::Id(id.to_owned()))
  }

  fn parent(
    &self,
    reg_ref: &dyn LocalRegistryReference,
  ) -> Result<LatestRegistryReference, RegistryError> {
    let new_unknown_parent_error = || RegistryError::UnknownParent {
      name: reg_ref.name().cloned().unwrap_or("<root>".to_string()),
    };
    let parent_id = reg_ref.parent().ok_or(new_unknown_parent_error())?;
    self
      .find_id(parent_id)
      .cloned()
      .ok_or(new_unknown_parent_error())
  }

  fn compute_path(&self, reg_ref: &dyn LocalRegistryReference) -> Result<String, RegistryError> {
    if reg_ref.is_root() {
      return Ok(String::new());
    }
    let record_name = reg_ref.name().expect("non-root record had no name");
    let path = match self.parent(reg_ref)? {
      LatestRegistryReference::Root => record_name.to_owned(),
      parent => format!("{}.{}", self.compute_path(&parent)?, record_name),
    };
    Ok(path)
  }

  pub fn add_enumeration_frozen(
    &mut self,
    id: Uuid,
    tag: Version,
    enumeration: EnumerationFrozen,
  ) -> Result<(), RegistryError> {
    let enumeration = Rc::new(enumeration);
    let reg_ref = FrozenRegistryReference::Enumeration {
      id: id.to_owned(),
      record: enumeration.to_owned(),
    };
    let path = self.compute_path(&reg_ref)?;
    let mut new_entries = HashMap::new();
    let mut new_mappings = HashMap::new();
    add_frozen_index_entry(
      &mut new_entries,
      Selector::Id(id.to_owned()),
      tag.to_owned(),
      reg_ref.to_owned(),
    )?;
    add_frozen_index_entry(
      &mut new_entries,
      Selector::Path(path.to_owned()),
      tag.to_owned(),
      reg_ref.to_owned(),
    )?;
    add_mapping(&mut new_mappings, path.to_owned(), id.to_owned())?;
    for (sub_id, variant) in &enumeration.variants {
      let sub_ref = FrozenRegistryReference::Variant {
        id: sub_id.to_owned(),
        parent_id: id.to_owned(),
        parent_record: enumeration.to_owned(),
      };
      let sub_path = format!("{}.{}", path, variant.name);
      add_frozen_index_entry(
        &mut new_entries,
        Selector::Id(sub_id.to_owned()),
        tag.to_owned(),
        sub_ref.to_owned(),
      )?;
      add_frozen_index_entry(
        &mut new_entries,
        Selector::Path(sub_path.to_owned()),
        tag.to_owned(),
        sub_ref.to_owned(),
      )?;
      add_mapping(&mut new_mappings, sub_path.to_owned(), sub_id.to_owned())?;
    }
    add_frozen_index_entries(
      &mut self.frozen_indexed,
      new_entries,
      &mut self.path_to_ids,
      new_mappings,
    )?;
    self.frozen_enumerations.push(enumeration.to_owned());
    Ok(())
  }

  pub fn add_structure_frozen(
    &mut self,
    id: Uuid,
    tag: Version,
    structure: StructureFrozen,
  ) -> Result<(), RegistryError> {
    let structure = Rc::new(structure);
    let reg_ref = FrozenRegistryReference::Structure {
      id: id.to_owned(),
      record: structure.to_owned(),
    };
    let path = self.compute_path(&reg_ref)?;
    let mut new_entries = HashMap::new();
    let mut new_mappings = HashMap::new();
    add_frozen_index_entry(
      &mut new_entries,
      Selector::Id(id.to_owned()),
      tag.to_owned(),
      reg_ref.to_owned(),
    )?;
    add_frozen_index_entry(
      &mut new_entries,
      Selector::Path(path.to_owned()),
      tag.to_owned(),
      reg_ref.to_owned(),
    )?;
    add_mapping(&mut new_mappings, path.to_owned(), id.to_owned())?;
    for (sub_id, field) in &structure.fields {
      let sub_ref = FrozenRegistryReference::Field {
        id: sub_id.to_owned(),
        parent_id: id.to_owned(),
        parent_record: structure.to_owned(),
      };
      let sub_path = format!("{}.{}", path, field.name);
      add_frozen_index_entry(
        &mut new_entries,
        Selector::Id(sub_id.to_owned()),
        tag.to_owned(),
        sub_ref.to_owned(),
      )?;
      add_frozen_index_entry(
        &mut new_entries,
        Selector::Path(sub_path.to_owned()),
        tag.to_owned(),
        sub_ref.to_owned(),
      )?;
      add_mapping(&mut new_mappings, sub_path.to_owned(), sub_id.to_owned())?;
    }
    add_frozen_index_entries(
      &mut self.frozen_indexed,
      new_entries,
      &mut self.path_to_ids,
      new_mappings,
    )?;
    self.frozen_structures.push(structure.to_owned());
    Ok(())
  }

  pub fn add_module_frozen(
    &mut self,
    id: Uuid,
    tag: Version,
    module: ModuleFrozen,
  ) -> Result<(), RegistryError> {
    let module: Rc<ModuleFrozen> = Rc::new(module);
    let reg_ref = FrozenRegistryReference::Module {
      id: id.to_owned(),
      record: module.to_owned(),
    };
    let path = self.compute_path(&reg_ref)?;
    let mut new_entries = HashMap::new();
    let mut new_mappings = HashMap::new();
    add_frozen_index_entry(
      &mut new_entries,
      Selector::Id(id.to_owned()),
      tag.to_owned(),
      reg_ref.to_owned(),
    )?;
    add_frozen_index_entry(
      &mut new_entries,
      Selector::Path(path.to_owned()),
      tag.to_owned(),
      reg_ref.to_owned(),
    )?;
    add_mapping(&mut new_mappings, path.to_owned(), id.to_owned())?;
    for (sub_id, export) in &module.exports {
      match export.kind {
        semio_record::module::v0::frozen::ExportKind::Function(_) => {
          let sub_ref = FrozenRegistryReference::Function {
            id: sub_id.to_owned(),
            parent_id: id.to_owned(),
            parent_record: module.to_owned(),
          };
          let sub_path = format!("{}.{}", path, export.name);
          add_frozen_index_entry(
            &mut new_entries,
            Selector::Id(sub_id.to_owned()),
            tag.to_owned(),
            sub_ref.to_owned(),
          )?;
          add_frozen_index_entry(
            &mut new_entries,
            Selector::Path(sub_path.to_owned()),
            tag.to_owned(),
            sub_ref.to_owned(),
          )?;
          add_mapping(&mut new_mappings, sub_path.to_owned(), sub_id.to_owned())?;
        }
      }
    }
    add_frozen_index_entries(
      &mut self.frozen_indexed,
      new_entries,
      &mut self.path_to_ids,
      new_mappings,
    )?;
    self.frozen_modules.push(module.to_owned());
    Ok(())
  }
}

#[async_trait]
impl Freezer for LocalRegistry {
  type Error = RegistryError;
  async fn freeze(&self, reference: &UnfrozenReference) -> Result<FrozenReference, Self::Error> {
    let selector = Selector::Id(reference.id.to_owned());
    let version = self
      .frozen_indexed
      .get(&selector)
      .ok_or(RegistryError::no_such_record(&selector))?
      .iter()
      .rev()
      .find(|(version, _)| {
        if let Some(version_req) = &reference.version_req.0 {
          version_req.matches(version)
        } else {
          true
        }
      })
      .ok_or(RegistryError::NoSuchVersion {
        selector,
        version_req: reference.version_req.0.clone().unwrap_or_default(),
      })?
      .0
      .to_owned();
    Ok(FrozenReference {
      id: reference.id.clone(),
      version: semio_record::record::Version(version),
    })
  }
}

fn check_frozen_index_entry_vacant(
  index: &mut HashMap<Selector, BTreeMap<Version, FrozenRegistryReference>>,
  selector: &Selector,
  version: &Version,
) -> Result<(), RegistryError> {
  if let Some(version_index) = index.get(selector) {
    if version_index.contains_key(version) {
      Err(RegistryError::DuplicateVersion {
        selector: selector.to_owned(),
        version: version.to_owned(),
      })
    } else {
      Ok(())
    }
  } else {
    Ok(())
  }
}

fn add_frozen_index_entry(
  index: &mut HashMap<Selector, BTreeMap<Version, FrozenRegistryReference>>,
  selector: Selector,
  version: Version,
  reg_ref: FrozenRegistryReference,
) -> Result<(), RegistryError> {
  match index.entry(selector.to_owned()) {
    hash_map::Entry::Occupied(mut entry) => {
      add_frozen_version_entry(entry.get_mut(), &selector, version, reg_ref)?
    }
    hash_map::Entry::Vacant(entry) => {
      add_frozen_version_entry(entry.insert(BTreeMap::new()), &selector, version, reg_ref)?
    }
  };
  Ok(())
}

fn add_frozen_version_entry<'a>(
  version_index: &'a mut BTreeMap<Version, FrozenRegistryReference>,
  selector: &Selector,
  version: Version,
  reg_ref: FrozenRegistryReference,
) -> Result<&'a mut FrozenRegistryReference, RegistryError> {
  match version_index.entry(version.to_owned()) {
    btree_map::Entry::Occupied(_) => Err(RegistryError::DuplicateVersion {
      selector: selector.to_owned(),
      version,
    }),
    btree_map::Entry::Vacant(entry) => Ok(entry.insert(reg_ref)),
  }
}

fn add_frozen_index_entries(
  index: &mut HashMap<Selector, BTreeMap<Version, FrozenRegistryReference>>,
  entries: HashMap<Selector, BTreeMap<Version, FrozenRegistryReference>>,
  path_to_ids: &mut HashMap<String, Uuid>,
  mappings: HashMap<String, Uuid>,
) -> Result<(), RegistryError> {
  // Check that transaction is valid.
  for (selector, version_index) in &entries {
    for (version, _) in version_index {
      check_frozen_index_entry_vacant(index, selector, version)?;
    }
  }
  for (path, _) in &mappings {
    if path_to_ids.contains_key(path) {
      Err(RegistryError::DuplicateSelector {
        selector: Selector::Path(path.to_owned()),
      })?
    }
  }
  // Apply the transaction.
  for (selector, version_index) in entries {
    for (version, reg_ref) in version_index {
      add_frozen_index_entry(index, selector.to_owned(), version.to_owned(), reg_ref).expect(
        format!(
          "failed to add frozen entry at {}@{} despite integrity was checked",
          selector, version
        )
        .as_str(),
      );
    }
  }
  for (path, id) in mappings {
    path_to_ids.insert(path.to_owned(), id.to_owned());
  }
  Ok(())
}

fn add_mapping(
  path_to_ids: &mut HashMap<String, Uuid>,
  path: String,
  id: Uuid,
) -> Result<(), RegistryError> {
  let entry = match path_to_ids.entry(path.to_owned()) {
    hash_map::Entry::Occupied(_) => {
      return Err(RegistryError::DuplicateSelector {
        selector: Selector::Path(path.to_owned()),
      })
    }
    hash_map::Entry::Vacant(entry) => entry,
  };
  entry.insert(id);
  Ok(())
}

pub const ROOT_ID: Uuid = Uuid::from_bytes([
  0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00,
]);

#[cfg(test)]
mod tests {
  use super::{LocalRegistry, ROOT_ID};
  use crate::{EditableRegistry, EnumerationPublic, ModulePublic};
  use semio_record::{
    enumeration::v0::unfrozen::EnumerationVariant,
    module::v0::unfrozen::{Export, ExportKind, Function},
    record::UnfrozenReference,
    ty::{Primitive, PrimitiveKind, UnfrozenScalar, UnfrozenTy},
  };
  use semver::{Version, VersionReq};
  use std::collections::{BTreeMap, HashMap};
  use uuid::Uuid;

  #[tokio::test]
  async fn add_status_enumeration_and_use_it_in_a_module() {
    let mut registry = LocalRegistry::new();

    let status = EnumerationPublic {
      name: "Status".to_owned(),
      parent: ROOT_ID,
      variants: HashMap::from([(
        Uuid::new_v4(),
        EnumerationVariant {
          name: "Ok".to_owned(),
          ty: UnfrozenTy::Primitive(Primitive {
            kind: semio_record::ty::PrimitiveKind::Unit,
          }),
        },
      )]),
    };
    let enum_id = registry
      .add_enumeration(Uuid::new_v4(), status)
      .await
      .unwrap();

    let module = ModulePublic {
      parent: ROOT_ID,
      name: "node".to_owned(),
      exports: HashMap::from([(
        Uuid::new_v4(),
        Export {
          name: "succeed".to_owned(),
          kind: ExportKind::Function(Function {
            parameters: HashMap::new(),
            parameter_ordering: vec![],
            return_ty: UnfrozenTy::UnfrozenScalar(UnfrozenScalar {
              reference: UnfrozenReference {
                id: enum_id,
                version_req: semio_record::record::VersionReq(None),
              },
            }),
          }),
        },
      )]),
      executable: None,
      dependencies: vec![],
    };
    registry.add_module(Uuid::new_v4(), module).await.unwrap();
  }

  #[test]
  pub fn versions() {
    let enumeration = EnumerationPublic {
      name: "Status".to_string(),
      parent: ROOT_ID,
      variants: HashMap::from([
        (
          Uuid::new_v4(),
          EnumerationVariant {
            name: "Success".to_string(),
            ty: UnfrozenTy::Primitive(Primitive {
              kind: PrimitiveKind::Unit,
            }),
          },
        ),
        (
          Uuid::new_v4(),
          EnumerationVariant {
            name: "Failure".to_string(),
            ty: UnfrozenTy::Primitive(Primitive {
              kind: PrimitiveKind::Unit,
            }),
          },
        ),
        (
          Uuid::new_v4(),
          EnumerationVariant {
            name: "Running".to_string(),
            ty: UnfrozenTy::Primitive(Primitive {
              kind: PrimitiveKind::Unit,
            }),
          },
        ),
      ]),
    };
    let version = Version::parse("1.0.0").unwrap();
    let mut enumerations_by_version = BTreeMap::new();
    enumerations_by_version.insert(version.clone(), enumeration.clone());
    let version_req = VersionReq::parse("=1").unwrap();
    let (matched_version, matched_enumeration) = enumerations_by_version
      .iter()
      .rev()
      .find(|(v, _)| version_req.matches(*v))
      .unwrap();
    assert_eq!(*matched_version, version);
    assert_eq!(*matched_enumeration, enumeration);
  }
}
