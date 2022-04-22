use super::{add_mapping, reg_ref::LatestRegistryReference, LocalRegistry};
use crate::{
  EditableRegistry, EnumerationFrozen, EnumerationPublic, FolderPublic, Module, ModuleFrozen,
  ModulePublic, RegistryError, Structure, StructureFrozen, StructurePublic,
};
use async_trait::async_trait;
use semio_client::common::Selector;
use semio_record::{enumeration::v0::unfrozen::Enumeration, module::v0::unfrozen, record::Freeze};
use semver::Version;
use std::{
  collections::{hash_map, HashMap},
  rc::Rc,
};
use uuid::Uuid;

#[async_trait]
impl EditableRegistry for LocalRegistry {
  async fn add_enumeration(
    &mut self,
    id: Uuid,
    enumeration: EnumerationPublic,
  ) -> Result<Uuid, RegistryError> {
    let enumeration = Rc::new(enumeration);
    let reg_ref = LatestRegistryReference::Enumeration {
      id: id.to_owned(),
      record: enumeration.to_owned(),
    };
    let path = self.compute_path(&reg_ref)?;
    let mut new_entries = HashMap::new();
    let mut new_mappings = HashMap::new();
    add_latest_index_entry(
      &mut new_entries,
      Selector::Id(id.to_owned()),
      reg_ref.to_owned(),
    )?;
    add_latest_index_entry(
      &mut new_entries,
      Selector::Path(path.to_owned()),
      reg_ref.to_owned(),
    )?;
    add_mapping(&mut new_mappings, path.to_owned(), id.to_owned())?;
    for (sub_id, variant) in &enumeration.variants {
      let sub_ref = LatestRegistryReference::Variant {
        id: sub_id.to_owned(),
        parent_id: id.to_owned(),
        parent_record: enumeration.to_owned(),
      };
      let sub_path = format!("{}.{}", path, variant.name);
      add_latest_index_entry(
        &mut new_entries,
        Selector::Id(sub_id.to_owned()),
        sub_ref.to_owned(),
      )?;
      add_latest_index_entry(
        &mut new_entries,
        Selector::Path(sub_path.to_owned()),
        sub_ref.to_owned(),
      )?;
      add_mapping(&mut new_mappings, sub_path.to_owned(), sub_id.to_owned())?;
    }
    add_latest_index_entries(
      &mut self.latest_indexed,
      new_entries,
      &mut self.path_to_ids,
      new_mappings,
    )?;
    self.latest_enumerations.push(enumeration.to_owned());
    Ok(id)
  }

  async fn tag_enumeration(
    &mut self,
    id: Uuid,
    tag: Version,
    enumeration: Enumeration,
  ) -> Result<EnumerationFrozen, RegistryError> {
    let enumeration = enumeration.freeze(self).await?;
    self.add_enumeration_frozen(id, tag, enumeration.to_owned())?;
    Ok(enumeration)
  }

  async fn add_structure(
    &mut self,
    id: Uuid,
    structure: StructurePublic,
  ) -> Result<(), RegistryError> {
    let structure = Rc::new(structure);
    let reg_ref = LatestRegistryReference::Structure {
      id: id.to_owned(),
      record: structure.to_owned(),
    };
    let path = self.compute_path(&reg_ref)?;
    let mut new_entries = HashMap::new();
    let mut new_mappings = HashMap::new();
    add_latest_index_entry(
      &mut new_entries,
      Selector::Id(id.to_owned()),
      reg_ref.to_owned(),
    )?;
    add_latest_index_entry(
      &mut new_entries,
      Selector::Path(path.to_owned()),
      reg_ref.to_owned(),
    )?;
    add_mapping(&mut new_mappings, path.to_owned(), id.to_owned())?;
    for (sub_id, field) in &structure.fields {
      let sub_ref = LatestRegistryReference::Field {
        id: sub_id.to_owned(),
        parent_id: id.to_owned(),
        parent_record: structure.to_owned(),
      };
      let sub_path = format!("{}.{}", path, field.name);
      add_latest_index_entry(
        &mut new_entries,
        Selector::Id(sub_id.to_owned()),
        sub_ref.to_owned(),
      )?;
      add_latest_index_entry(
        &mut new_entries,
        Selector::Path(sub_path.to_owned()),
        sub_ref.to_owned(),
      )?;
      add_mapping(&mut new_mappings, sub_path.to_owned(), sub_id.to_owned())?;
    }
    add_latest_index_entries(
      &mut self.latest_indexed,
      new_entries,
      &mut self.path_to_ids,
      new_mappings,
    )?;
    self.latest_structures.push(structure.to_owned());
    Ok(())
  }

  async fn tag_structure(
    &mut self,
    id: Uuid,
    tag: Version,
    structure: Structure,
  ) -> Result<StructureFrozen, RegistryError> {
    let structure = structure.freeze(self).await?;
    self.add_structure_frozen(id, tag, structure.to_owned())?;
    Ok(structure)
  }

  async fn add_module(&mut self, id: Uuid, module: ModulePublic) -> Result<(), RegistryError> {
    let module = Rc::new(module);
    let reg_ref = LatestRegistryReference::Module {
      id: id.to_owned(),
      record: module.to_owned(),
    };
    let path = self.compute_path(&reg_ref)?;
    let mut new_entries = HashMap::new();
    let mut new_mappings = HashMap::new();
    add_latest_index_entry(
      &mut new_entries,
      Selector::Id(id.to_owned()),
      reg_ref.to_owned(),
    )?;
    add_latest_index_entry(
      &mut new_entries,
      Selector::Path(path.to_owned()),
      reg_ref.to_owned(),
    )?;
    add_mapping(&mut new_mappings, path.to_owned(), id.to_owned())?;
    for (sub_id, export) in &module.exports {
      match export.kind {
        unfrozen::ExportKind::Function(_) => {
          let sub_ref = LatestRegistryReference::Function {
            id: sub_id.to_owned(),
            parent_id: id.to_owned(),
            parent_record: module.to_owned(),
          };
          let sub_path = format!("{}.{}", path, export.name);
          add_latest_index_entry(
            &mut new_entries,
            Selector::Id(sub_id.to_owned()),
            sub_ref.to_owned(),
          )?;
          add_latest_index_entry(
            &mut new_entries,
            Selector::Path(sub_path.to_owned()),
            sub_ref.to_owned(),
          )?;
          add_mapping(&mut new_mappings, sub_path.to_owned(), sub_id.to_owned())?;
        }
      }
    }
    add_latest_index_entries(
      &mut self.latest_indexed,
      new_entries,
      &mut self.path_to_ids,
      new_mappings,
    )?;
    self.latest_modules.push(module.to_owned());
    Ok(())
  }

  async fn tag_module(
    &mut self,
    id: Uuid,
    tag: Version,
    module: Module,
  ) -> Result<ModuleFrozen, RegistryError> {
    let module: ModuleFrozen = module.freeze(self).await?;
    self.add_module_frozen(id, tag, module.to_owned())?;
    Ok(module)
  }

  async fn add_folder(&mut self, id: Uuid, folder: FolderPublic) -> Result<(), RegistryError> {
    let folder = Rc::new(folder);
    let reg_ref = LatestRegistryReference::Folder {
      id: id.to_owned(),
      record: folder.to_owned(),
    };
    let path = self.compute_path(&reg_ref)?;
    let mut new_entries = HashMap::new();
    let mut new_mappings = HashMap::new();
    add_latest_index_entry(
      &mut new_entries,
      Selector::Id(id.to_owned()),
      reg_ref.to_owned(),
    )?;
    add_latest_index_entry(
      &mut new_entries,
      Selector::Path(path.to_owned()),
      reg_ref.to_owned(),
    )?;
    add_mapping(&mut new_mappings, path.to_owned(), id.to_owned())?;
    add_latest_index_entries(
      &mut self.latest_indexed,
      new_entries,
      &mut self.path_to_ids,
      new_mappings,
    )?;
    self.latest_folders.push(folder.to_owned());
    Ok(())
  }
}

fn check_latest_index_entry_vacant(
  index: &mut HashMap<Selector, LatestRegistryReference>,
  selector: &Selector,
) -> Result<(), RegistryError> {
  if index.contains_key(selector) {
    Err(RegistryError::DuplicateSelector {
      selector: selector.to_owned(),
    })
  } else {
    Ok(())
  }
}

fn get_latest_index_entry(
  index: &mut HashMap<Selector, LatestRegistryReference>,
  selector: Selector,
) -> Result<hash_map::VacantEntry<Selector, LatestRegistryReference>, RegistryError> {
  match index.entry(selector.to_owned()) {
    hash_map::Entry::Occupied(_) => return Err(RegistryError::DuplicateSelector { selector }),
    hash_map::Entry::Vacant(entry) => Ok(entry),
  }
}

fn add_latest_index_entry(
  index: &mut HashMap<Selector, LatestRegistryReference>,
  selector: Selector,
  reg_ref: LatestRegistryReference,
) -> Result<(), RegistryError> {
  get_latest_index_entry(index, selector)?.insert(reg_ref);
  Ok(())
}

/// Adds the entries to the index of latest records.
/// If any of the insertions should fail,
/// none of the insertions will be performed.
fn add_latest_index_entries(
  index: &mut HashMap<Selector, LatestRegistryReference>,
  entries: HashMap<Selector, LatestRegistryReference>,
  path_to_ids: &mut HashMap<String, Uuid>,
  mappings: HashMap<String, Uuid>,
) -> Result<(), RegistryError> {
  // Check that transaction is valid.
  for (selector, _) in &entries {
    check_latest_index_entry_vacant(index, selector)?;
  }
  for (path, _) in &mappings {
    if path_to_ids.contains_key(path) {
      Err(RegistryError::DuplicateSelector {
        selector: Selector::Path(path.to_owned()),
      })?
    }
  }
  // Apply the transaction.
  for (selector, reg_ref) in entries {
    add_latest_index_entry(index, selector, reg_ref)?;
  }
  for (path, id) in mappings {
    path_to_ids.insert(path.to_owned(), id.to_owned());
  }
  Ok(())
}
