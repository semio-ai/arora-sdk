use super::{reg_ref::FrozenRegistryReference, LocalRegistry};
use crate::{
  EditableRegistry, EnumerationFrozen, FolderPublic, Module, ModuleFrozen, RegistryError,
  Structure, StructureFrozen,
};
use async_trait::async_trait;
use semio_client::common::Selector;
use semio_record::{enumeration::v0::unfrozen::Enumeration, record::Freeze};
use semver::Version;
use std::{
  collections::{btree_map, hash_map, BTreeMap, HashMap},
  rc::Rc,
};
use uuid::Uuid;

#[async_trait]
impl EditableRegistry for LocalRegistry {
  async fn add_enumeration(
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
      &mut self.indexed,
      new_entries,
      &mut self.path_to_ids,
      new_mappings,
    )?;
    self.enumerations.push(enumeration.to_owned());
    Ok(())
  }

  async fn tag_enumeration(
    &mut self,
    id: Uuid,
    tag: Version,
    enumeration: Enumeration,
  ) -> Result<EnumerationFrozen, RegistryError> {
    let enumeration = enumeration.freeze(self).await?;
    self
      .add_enumeration(id, tag, enumeration.to_owned())
      .await?;
    Ok(enumeration)
  }

  async fn add_structure(
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
      &mut self.indexed,
      new_entries,
      &mut self.path_to_ids,
      new_mappings,
    )?;
    self.structures.push(structure.to_owned());
    Ok(())
  }

  async fn tag_structure(
    &mut self,
    id: Uuid,
    tag: Version,
    structure: Structure,
  ) -> Result<StructureFrozen, RegistryError> {
    let structure = structure.freeze(self).await?;
    self.add_structure(id, tag, structure.to_owned()).await?;
    Ok(structure)
  }

  async fn add_module(
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
      &mut self.indexed,
      new_entries,
      &mut self.path_to_ids,
      new_mappings,
    )?;
    self.modules.push(module.to_owned());
    Ok(())
  }

  async fn tag_module(
    &mut self,
    id: Uuid,
    tag: Version,
    module: Module,
  ) -> Result<ModuleFrozen, RegistryError> {
    let module: ModuleFrozen = module.freeze(self).await?;
    self.add_module(id, tag, module.to_owned()).await?;
    Ok(module)
  }

  async fn add_folder(&mut self, id: Uuid, folder: FolderPublic) -> Result<(), RegistryError> {
    let folder = Rc::new(folder);
    let reg_ref = FrozenRegistryReference::Folder {
      id: id.to_owned(),
      record: folder.to_owned(),
    };
    let path = self.compute_path(&reg_ref)?;
    let mut new_entries = HashMap::new();
    let mut new_mappings = HashMap::new();
    add_frozen_index_entry(
      &mut new_entries,
      Selector::Id(id.to_owned()),
      Version::new(0, 0, 0),
      reg_ref.to_owned(),
    )?;
    add_frozen_index_entry(
      &mut new_entries,
      Selector::Path(path.to_owned()),
      Version::new(0, 0, 0),
      reg_ref.to_owned(),
    )?;
    add_mapping(&mut new_mappings, path.to_owned(), id.to_owned())?;
    add_frozen_index_entries(
      &mut self.indexed,
      new_entries,
      &mut self.path_to_ids,
      new_mappings,
    )?;
    self.folders.push(folder.to_owned());
    Ok(())
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

/// Adds the entries to the index of frozen records.
/// If any of the insertions should fail,
/// none of the insertions will be performed.
fn add_frozen_index_entries(
  index: &mut HashMap<Selector, BTreeMap<Version, FrozenRegistryReference>>,
  entries: HashMap<Selector, BTreeMap<Version, FrozenRegistryReference>>,
  path_to_ids: &mut HashMap<String, Uuid>,
  mappings: HashMap<String, Uuid>,
) -> Result<(), RegistryError> {
  // Check that transaction is valid.
  for (selector, version_index) in &entries {
    for version in version_index.keys() {
      check_frozen_index_entry_vacant(index, selector, version)?;
    }
  }
  for path in mappings.keys() {
    if path_to_ids.contains_key(path) {
      Err(RegistryError::DuplicateSelector {
        selector: Selector::Path(path.to_owned()),
      })?
    }
  }
  // Apply the transaction.
  for (selector, version_index) in entries {
    for (version, reg_ref) in version_index {
      add_frozen_index_entry(index, selector.to_owned(), version.to_owned(), reg_ref).unwrap_or_else(|_| panic!("failed to add frozen entry at {}@{} despite integrity was checked",
          selector, version));
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
