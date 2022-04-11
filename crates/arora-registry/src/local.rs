use crate::{
  get_primitive, EditableRegistry, EnumerationPublic, FolderPublic, ModulePublic, ReadableRegistry,
  RegistryError, StructurePublic, TypeDefinition,
};
use async_trait::async_trait;
use semio_client::common::{EntityType, Selector};
use semio_record::module::v0::unfrozen::ExportKind;
use std::{
  collections::{
    hash_map::{Entry, VacantEntry},
    HashMap,
  },
  rc::Rc,
};
use uuid::Uuid;

/// A [`LocalRegistry`] supports the addition of [`Structure`], [`Enumeration`] and [`Module`]
/// on the fly. It provides a local index to look them up fast
/// by [`Uuid'] or by path ([`String`]).
/// It can be used as a local cache of a remote registry accessed using [`semio_client`].
/// It provides an absolute root available for any entity,
/// with the identifier [`ROOT_ID`].
pub struct LocalRegistry {
  enumerations: Vec<Rc<EnumerationPublic>>,
  structures: Vec<Rc<StructurePublic>>,
  modules: Vec<Rc<ModulePublic>>,
  folders: Vec<Rc<FolderPublic>>,
  indexed: HashMap<Selector, RegistryReference>,
  path_to_ids: HashMap<String, Uuid>,
}

/// A reference to an entity stored in a [`LocalRegistry`].
#[derive(Clone)]
pub enum RegistryReference {
  Enumeration(Rc<EnumerationPublic>),
  Variant(Rc<EnumerationPublic>, Uuid),
  Structure(Rc<StructurePublic>),
  Field(Rc<StructurePublic>, Uuid),
  Module(Rc<ModulePublic>),
  Function(Rc<ModulePublic>, Uuid),
  Folder(Rc<FolderPublic>),
  Root,
}

/// A local registry of types ([`Structure`] or [`Enumeration`])
/// and [`Module`]), indexed for an efficient lookup by identifier
/// or by name.
impl LocalRegistry {
  pub fn new() -> Self {
    Self {
      enumerations: Vec::new(),
      structures: Vec::new(),
      modules: Vec::new(),
      folders: Vec::new(),
      indexed: HashMap::from([(Selector::Id(ROOT_ID.to_owned()), RegistryReference::Root)]),
      path_to_ids: HashMap::new(),
    }
  }

  pub fn find(&self, selector: &Selector) -> Option<&RegistryReference> {
    self.indexed.get(selector)
  }

  pub fn find_id(&self, id: &Uuid) -> Option<&RegistryReference> {
    self.find(&Selector::Id(id.to_owned()))
  }

  fn parent(&self, reg_ref: &RegistryReference) -> Result<RegistryReference, RegistryError> {
    let parent_ref = match reg_ref {
      RegistryReference::Enumeration(entity) => self.find_id(&entity.parent).cloned(),
      RegistryReference::Variant(entity, _) => {
        Some(RegistryReference::Enumeration(entity.to_owned()))
      }
      RegistryReference::Structure(entity) => self.find_id(&entity.parent).cloned(),
      RegistryReference::Field(entity, _) => Some(RegistryReference::Structure(entity.to_owned())),
      RegistryReference::Module(entity) => self.find_id(&entity.parent).cloned(),
      RegistryReference::Function(entity, _) => Some(RegistryReference::Module(entity.to_owned())),
      RegistryReference::Folder(entity) => self.find_id(&entity.parent).cloned(),
      RegistryReference::Root => None,
    };
    parent_ref.ok_or(RegistryError::UnknownParent {
      name: reg_ref.name().cloned().unwrap_or("<root>".to_string()),
    })
  }

  fn compute_path(&self, reg_ref: &RegistryReference) -> Result<String, RegistryError> {
    let path = match reg_ref {
      RegistryReference::Root => String::new(),
      reg_ref => {
        let entity_name = reg_ref.name().expect("non-root entity had no name");
        match self.parent(reg_ref)? {
          RegistryReference::Root => entity_name.to_owned(),
          parent => format!("{}.{}", self.compute_path(&parent)?, entity_name),
        }
      }
    };
    Ok(path)
  }
}

#[async_trait(?Send)]
impl ReadableRegistry for LocalRegistry {
  async fn get_type(&mut self, selector: &Selector) -> Result<TypeDefinition, RegistryError> {
    if let Some(primitive_kind) = get_primitive(selector) {
      return Ok(TypeDefinition::Primitive(primitive_kind));
    }
    let reg_ref = self
      .indexed
      .get(selector)
      .ok_or(RegistryError::NoSuchEntity {
        selector: selector.to_owned(),
      })?;
    match reg_ref {
      RegistryReference::Enumeration(entity) => {
        Ok(TypeDefinition::Enumeration(entity.as_ref().clone()))
      }
      RegistryReference::Structure(entity) => {
        Ok(TypeDefinition::Structure(entity.as_ref().clone()))
      }
      _ => Err(RegistryError::NotAType {
        selector: selector.to_owned(),
      }),
    }
  }

  async fn get_module(&mut self, selector: &Selector) -> Result<ModulePublic, RegistryError> {
    let reg_ref = self
      .indexed
      .get(selector)
      .ok_or(RegistryError::NoSuchEntity {
        selector: selector.to_owned(),
      })?;
    match reg_ref {
      RegistryReference::Module(entity) => Ok(entity.as_ref().clone()),
      _ => Err(RegistryError::NotAModule {
        selector: selector.to_owned(),
      }),
    }
  }

  async fn resolve_path(&mut self, path: &String) -> Result<Uuid, RegistryError> {
    Ok(
      self
        .path_to_ids
        .get(path)
        .ok_or(RegistryError::NoSuchEntity {
          selector: Selector::Path(path.to_owned()),
        })?
        .clone(),
    )
  }

  async fn resolve_id(&mut self, id: &Uuid) -> Result<String, RegistryError> {
    self.compute_path(self.indexed.get(&Selector::Id(id.to_owned())).ok_or(
      RegistryError::NoSuchEntity {
        selector: Selector::Id(id.to_owned()),
      },
    )?)
  }

  async fn type_of(&mut self, selector: &Selector) -> Result<EntityType, RegistryError> {
    self
      .indexed
      .get(selector)
      .map(|reg_ref| match reg_ref {
        RegistryReference::Enumeration(_) => EntityType::Enumeration,
        RegistryReference::Variant(_, _) => EntityType::Unknown,
        RegistryReference::Structure(_) => EntityType::Structure,
        RegistryReference::Field(_, _) => EntityType::Unknown,
        RegistryReference::Module(_) => EntityType::Module,
        RegistryReference::Function(_, _) => EntityType::Unknown,
        RegistryReference::Folder(_) => EntityType::Folder,
        RegistryReference::Root => EntityType::Unknown,
      })
      .ok_or(RegistryError::NoSuchEntity {
        selector: selector.to_owned(),
      })
  }
}

#[async_trait(?Send)]
impl EditableRegistry for LocalRegistry {
  async fn add_enumeration(
    &mut self,
    id: Uuid,
    enumeration: EnumerationPublic,
  ) -> Result<Uuid, RegistryError> {
    let enumeration = Rc::new(enumeration);
    let reg_ref = RegistryReference::Enumeration(enumeration.to_owned());
    let path = self.compute_path(&reg_ref)?;
    let mut new_entries = HashMap::new();
    let mut new_mappings = HashMap::new();
    add_index_entry(
      &mut new_entries,
      Selector::Id(id.to_owned()),
      reg_ref.to_owned(),
    )?;
    add_index_entry(
      &mut new_entries,
      Selector::Path(path.to_owned()),
      reg_ref.to_owned(),
    )?;
    add_mapping(&mut new_mappings, path.to_owned(), id.to_owned())?;
    for (sub_id, variant) in &enumeration.variants {
      let sub_ref = RegistryReference::Variant(enumeration.to_owned(), sub_id.to_owned());
      let sub_path = format!("{}.{}", path, variant.name);
      add_index_entry(
        &mut new_entries,
        Selector::Id(sub_id.to_owned()),
        sub_ref.to_owned(),
      )?;
      add_index_entry(
        &mut new_entries,
        Selector::Path(sub_path.to_owned()),
        sub_ref.to_owned(),
      )?;
      add_mapping(&mut new_mappings, sub_path.to_owned(), sub_id.to_owned())?;
    }
    add_index_entries(
      &mut self.indexed,
      new_entries,
      &mut self.path_to_ids,
      new_mappings,
    )?;
    self.enumerations.push(enumeration.to_owned());
    Ok(id)
  }

  async fn add_structure(
    &mut self,
    id: Uuid,
    structure: StructurePublic,
  ) -> Result<(), RegistryError> {
    let structure = Rc::new(structure);
    let reg_ref = RegistryReference::Structure(structure.to_owned());
    let path = self.compute_path(&reg_ref)?;
    let mut new_entries = HashMap::new();
    let mut new_mappings = HashMap::new();
    add_index_entry(
      &mut new_entries,
      Selector::Id(id.to_owned()),
      reg_ref.to_owned(),
    )?;
    add_index_entry(
      &mut new_entries,
      Selector::Path(path.to_owned()),
      reg_ref.to_owned(),
    )?;
    add_mapping(&mut new_mappings, path.to_owned(), id.to_owned())?;
    for (sub_id, field) in &structure.fields {
      let sub_ref = RegistryReference::Field(structure.to_owned(), sub_id.to_owned());
      let sub_path = format!("{}.{}", path, field.name);
      add_index_entry(
        &mut new_entries,
        Selector::Id(sub_id.to_owned()),
        sub_ref.to_owned(),
      )?;
      add_index_entry(
        &mut new_entries,
        Selector::Path(sub_path.to_owned()),
        sub_ref.to_owned(),
      )?;
      add_mapping(&mut new_mappings, sub_path.to_owned(), sub_id.to_owned())?;
    }
    add_index_entries(
      &mut self.indexed,
      new_entries,
      &mut self.path_to_ids,
      new_mappings,
    )?;
    self.structures.push(structure.to_owned());
    Ok(())
  }

  async fn add_module(&mut self, id: Uuid, module: ModulePublic) -> Result<(), RegistryError> {
    let module = Rc::new(module);
    let reg_ref = RegistryReference::Module(module.to_owned());
    let path = self.compute_path(&reg_ref)?;
    let mut new_entries = HashMap::new();
    let mut new_mappings = HashMap::new();
    add_index_entry(
      &mut new_entries,
      Selector::Id(id.to_owned()),
      reg_ref.to_owned(),
    )?;
    add_index_entry(
      &mut new_entries,
      Selector::Path(path.to_owned()),
      reg_ref.to_owned(),
    )?;
    add_mapping(&mut new_mappings, path.to_owned(), id.to_owned())?;
    for (sub_id, export) in &module.exports {
      match export.kind {
        ExportKind::Function(_) => {
          let sub_ref = RegistryReference::Function(module.to_owned(), sub_id.to_owned());
          let sub_path = format!("{}.{}", path, export.name);
          add_index_entry(
            &mut new_entries,
            Selector::Id(sub_id.to_owned()),
            sub_ref.to_owned(),
          )?;
          add_index_entry(
            &mut new_entries,
            Selector::Path(sub_path.to_owned()),
            sub_ref.to_owned(),
          )?;
          add_mapping(&mut new_mappings, sub_path.to_owned(), sub_id.to_owned())?;
        }
      }
    }
    add_index_entries(
      &mut self.indexed,
      new_entries,
      &mut self.path_to_ids,
      new_mappings,
    )?;
    self.modules.push(module.to_owned());
    Ok(())
  }

  async fn add_folder(&mut self, id: Uuid, folder: FolderPublic) -> Result<(), RegistryError> {
    let folder = Rc::new(folder);
    let reg_ref = RegistryReference::Folder(folder.to_owned());
    let path = self.compute_path(&reg_ref)?;
    let mut new_entries = HashMap::new();
    let mut new_mappings = HashMap::new();
    add_index_entry(
      &mut new_entries,
      Selector::Id(id.to_owned()),
      reg_ref.to_owned(),
    )?;
    add_index_entry(
      &mut new_entries,
      Selector::Path(path.to_owned()),
      reg_ref.to_owned(),
    )?;
    add_mapping(&mut new_mappings, path.to_owned(), id.to_owned())?;
    add_index_entries(
      &mut self.indexed,
      new_entries,
      &mut self.path_to_ids,
      new_mappings,
    )?;
    self.folders.push(folder.to_owned());
    Ok(())
  }
}

impl<'a> RegistryReference {
  fn name(&'a self) -> Option<&'a String> {
    Some(match self {
      RegistryReference::Enumeration(entity) => &entity.name,
      RegistryReference::Variant(entity, variant_id) => {
        &entity
          .variants
          .get(variant_id)
          .expect("looking up a variant id not known to its enumeration")
          .name
      }
      RegistryReference::Structure(entity) => &entity.name,
      RegistryReference::Field(entity, field_id) => {
        &entity
          .fields
          .get(field_id)
          .expect("looking up a field id not known to its structure")
          .name
      }
      RegistryReference::Module(entity) => &entity.name,
      RegistryReference::Function(entity, export_id) => {
        &entity
          .exports
          .get(export_id)
          .expect("looking up an export id not known to its module")
          .name
      }
      RegistryReference::Folder(entity) => &entity.name,
      RegistryReference::Root => return None,
    })
  }
}

fn check_index_entry_vacant(
  index: &mut HashMap<Selector, RegistryReference>,
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

fn get_index_entry(
  index: &mut HashMap<Selector, RegistryReference>,
  selector: Selector,
) -> Result<VacantEntry<Selector, RegistryReference>, RegistryError> {
  match index.entry(selector.to_owned()) {
    Entry::Occupied(_) => return Err(RegistryError::DuplicateSelector { selector }),
    Entry::Vacant(entry) => Ok(entry),
  }
}

fn add_index_entry(
  index: &mut HashMap<Selector, RegistryReference>,
  selector: Selector,
  reg_ref: RegistryReference,
) -> Result<(), RegistryError> {
  get_index_entry(index, selector)?.insert(reg_ref);
  Ok(())
}

fn add_mapping(
  path_to_ids: &mut HashMap<String, Uuid>,
  path: String,
  id: Uuid,
) -> Result<(), RegistryError> {
  let entry = match path_to_ids.entry(path.to_owned()) {
    Entry::Occupied(_) => {
      return Err(RegistryError::DuplicateSelector {
        selector: Selector::Path(path.to_owned()),
      })
    }
    Entry::Vacant(entry) => entry,
  };
  entry.insert(id);
  Ok(())
}

/// Adds the entries to the index.
/// If any of the insertions should fail,
/// none of the insertions will be performed.
fn add_index_entries(
  index: &mut HashMap<Selector, RegistryReference>,
  entries: HashMap<Selector, RegistryReference>,
  path_to_ids: &mut HashMap<String, Uuid>,
  mappings: HashMap<String, Uuid>,
) -> Result<(), RegistryError> {
  // Check that transaction is valid.
  for (selector, _) in &entries {
    check_index_entry_vacant(index, selector)?;
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
    add_index_entry(index, selector, reg_ref)?;
  }
  for (path, id) in mappings {
    path_to_ids.insert(path.to_owned(), id.to_owned());
  }
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
    record::{UnfrozenReference, VersionReq},
    ty::{Primitive, UnfrozenScalar, UnfrozenTy},
  };
  use std::collections::HashMap;
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
                version_req: VersionReq::parse("*").unwrap(),
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
}
