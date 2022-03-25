use derive_more::Display;
use semio_client::common::Selector;
use semio_record::{
  enumeration::v0::frozen::Enumeration,
  module::v0::frozen::{ExportKind, Module},
  record::{Frozen, FrozenReference},
  structure::v0::frozen::Structure,
};
use std::{
  collections::{
    hash_map::{Entry, VacantEntry},
    HashMap, HashSet,
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
  enumerations: Vec<Rc<Enumeration>>,
  structures: Vec<Rc<Structure>>,
  modules: Vec<Rc<Module>>,
  indexed: HashMap<Selector, RegistryReference>,
}

/// A reference to an entity stored in a [`LocalRegistry`].
#[derive(Clone)]
pub enum RegistryReference {
  Enumeration(Rc<Enumeration>),
  Variant(Rc<Enumeration>, Uuid),
  Structure(Rc<Structure>),
  Field(Rc<Structure>, Uuid),
  Module(Rc<Module>),
  Function(Rc<Module>, Uuid),
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
      indexed: HashMap::from([(Selector::Id(ROOT_ID.to_owned()), RegistryReference::Root)]),
    }
  }

  /// Adds an [`Enumeration`] to the registry.
  /// Its parent must be found in the registry.
  /// Its name must be unique under the given parent.
  /// Its identifier must be unique in the registry.
  /// All variants will be registered too.
  /// Dependent types must be found in the registry.
  /// Returns the identifier under which the enumeration
  /// was registered.
  pub fn add_enumeration(&mut self, enumeration: Enumeration) -> Result<Uuid, RegistryError> {
    self.check_dependencies_known(&enumeration)?;
    let enumeration = Rc::new(enumeration);
    let reg_ref = RegistryReference::Enumeration(enumeration.to_owned());
    let id = Uuid::new_v4();
    let path = self.compute_path(&reg_ref)?;
    let mut new_entries = HashMap::new();
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
    for (variant_id, variant) in &enumeration.variants {
      let variant_ref = RegistryReference::Variant(enumeration.to_owned(), variant_id.to_owned());
      let variant_path = format!("{}.{}", path, variant.name);
      add_index_entry(
        &mut new_entries,
        Selector::Id(variant_id.to_owned()),
        variant_ref.to_owned(),
      )?;
      add_index_entry(
        &mut new_entries,
        Selector::Path(variant_path),
        variant_ref.to_owned(),
      )?;
    }
    add_index_entries(&mut self.indexed, new_entries)?;
    self.enumerations.push(enumeration.to_owned());
    Ok(id)
  }

  /// Adds a [`Structure`] to the registry.
  /// Its parent must be found in the registry.
  /// Its name must be unique under the given parent.
  /// Its identifier must be unique in the registry.
  /// All fields will be registered too.
  /// Dependent types must be found in the registry.
  /// Returns the identifier under which the structure
  /// was registered.
  pub fn add_structure(&mut self, structure: Structure) -> Result<Uuid, RegistryError> {
    self.check_dependencies_known(&structure)?;
    let structure = Rc::new(structure);
    let reg_ref = RegistryReference::Structure(structure.to_owned());
    let id = Uuid::new_v4();
    let path = self.compute_path(&reg_ref)?;
    let mut new_entries = HashMap::new();
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
    for (field_id, field) in &structure.fields {
      let field_ref = RegistryReference::Field(structure.to_owned(), field_id.to_owned());
      let field_path = format!("{}.{}", path, field.name);
      add_index_entry(
        &mut new_entries,
        Selector::Id(field_id.to_owned()),
        field_ref.to_owned(),
      )?;
      add_index_entry(
        &mut new_entries,
        Selector::Path(field_path),
        field_ref.to_owned(),
      )?;
    }
    add_index_entries(&mut self.indexed, new_entries)?;
    self.structures.push(structure.to_owned());
    Ok(id)
  }

  /// Adds a [`Module`] to the registry.
  /// Its parent must be found in the registry.
  /// Its name must be unique under the given parent.
  /// Its identifier must be unique in the registry.
  /// All fields will be registered too.
  /// Dependent types must be found in the registry.
  /// Returns the identifier under which the module
  /// was registered.
  pub fn add_module(&mut self, module: Module) -> Result<Uuid, RegistryError> {
    self.check_dependencies_known(&module)?;
    let module = Rc::new(module);
    let reg_ref = RegistryReference::Module(module.to_owned());
    let id = Uuid::new_v4();
    let path = self.compute_path(&reg_ref)?;
    let mut new_entries = HashMap::new();
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
    for (export_id, export) in &module.exports {
      match export.kind {
        ExportKind::Function(_) => {
          let field_ref = RegistryReference::Function(module.to_owned(), export_id.to_owned());
          let field_path = format!("{}.{}", path, export.name);
          add_index_entry(
            &mut new_entries,
            Selector::Id(export_id.to_owned()),
            field_ref.to_owned(),
          )?;
          add_index_entry(
            &mut new_entries,
            Selector::Path(field_path),
            field_ref.to_owned(),
          )?;
        }
      }
    }
    add_index_entries(&mut self.indexed, new_entries)?;
    self.modules.push(module.to_owned());
    Ok(id)
  }

  pub fn find(&self, selector: &Selector) -> Option<&RegistryReference> {
    self.indexed.get(selector)
  }

  pub fn find_id(&self, id: &Uuid) -> Option<&RegistryReference> {
    self.find(&Selector::Id(id.to_owned()))
  }

  fn check_frozen_reference_known(
    &self,
    frozen_reference: &FrozenReference,
  ) -> Result<(), RegistryError> {
    self
      .find_id(&frozen_reference.id)
      .ok_or(RegistryError::UnknownDependency {
        selector: Selector::Id(frozen_reference.id.to_owned()),
      })
      .map(|_| ())
  }

  fn check_dependencies_known<T: Frozen>(&self, frozen: &T) -> Result<(), RegistryError> {
    let mut deps = HashSet::new();
    frozen.dependencies(&mut deps);
    for dep in deps {
      self.check_frozen_reference_known(dep)?;
    }
    Ok(())
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

/// Adds the entries to the index.
/// If any of the insertions should fail,
/// none of the insertions will be performed.
fn add_index_entries(
  index: &mut HashMap<Selector, RegistryReference>,
  entries: HashMap<Selector, RegistryReference>,
) -> Result<(), RegistryError> {
  for (selector, _) in &entries {
    check_index_entry_vacant(index, selector)?;
  }
  for (selector, reg_ref) in entries {
    add_index_entry(index, selector, reg_ref)?;
  }
  Ok(())
}

#[derive(Display, Debug)]
pub enum RegistryError {
  /// Entity being inserted has a parent defined, but it is unknown locally.
  #[display(fmt = "parent of entity \"{}\" is unknown to the registry", name)]
  UnknownParent { name: String },

  /// The name or identifier of the entity being added is already taken by another entity.
  #[display(
    fmt = "added entity's selector {} is already taken in the registry",
    selector
  )]
  DuplicateSelector { selector: Selector },

  /// Entity being inserted uses a dependency that is not known locally.
  #[display(
    fmt = "entity depends on \"{}\", which is unknown to the registry",
    selector
  )]
  UnknownDependency { selector: Selector },
}

impl std::error::Error for RegistryError {}

pub const ROOT_ID: Uuid = Uuid::from_bytes([
  0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00,
]);

#[cfg(test)]
mod tests {
  use std::collections::HashMap;

  use semio_record::{
    enumeration::v0::frozen::{Enumeration, EnumerationVariant},
    module::v0::frozen::{Export, ExportKind, Function, Module},
    record::{FrozenReference, Version},
    ty::{FrozenScalar, FrozenTy, Primitive},
  };
  use uuid::Uuid;

  use super::{LocalRegistry, ROOT_ID};

  #[test]
  fn add_status_enumeration_and_use_it_in_a_module() {
    let mut registry = LocalRegistry::new();

    let status = Enumeration {
      name: "Status".to_owned(),
      parent: ROOT_ID,
      variants: HashMap::from([(
        Uuid::new_v4(),
        EnumerationVariant {
          name: "Ok".to_owned(),
          ty: FrozenTy::Primitive(Primitive {
            kind: semio_record::ty::PrimitiveKind::Unit,
          }),
        },
      )]),
    };
    let enum_id = registry.add_enumeration(status).unwrap();

    let module = Module {
      parent: ROOT_ID,
      name: "node".to_owned(),
      exports: HashMap::from([(
        Uuid::new_v4(),
        Export {
          name: "succeed".to_owned(),
          kind: ExportKind::Function(Function {
            parameters: HashMap::new(),
            parameter_ordering: vec![],
            return_ty: FrozenTy::FrozenScalar(FrozenScalar {
              reference: FrozenReference {
                id: enum_id,
                version: Version(semver::Version::new(0, 0, 0)),
              },
            }),
          }),
        },
      )]),
      executable: None,
      dependencies: vec![],
    };
    registry.add_module(module).unwrap();
  }
}
