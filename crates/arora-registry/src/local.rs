use derive_more::Display;
use semio_client::common::Selector;
use semio_record::{
  enumeration::v0::frozen::Enumeration, module::v0::frozen::Module,
  structure::v0::frozen::Structure,
};
use std::{
  collections::{hash_map::Entry, HashMap},
  rc::Rc,
};
use uuid::Uuid;

/// A [`LocalRegistry`] supports the addition of [`Structure`], [`Enumeration`] and [`Module`]
/// on the fly. It provides a local index to look them up fast.
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
  Module(Rc<Module>),
  Root,
}

impl LocalRegistry {
  /// Add an [`Enumeration`] to the registry.
  /// Its parent must be found in the registry.
  /// Its name must be unique under the given parent.
  /// Its identifier must be unique in the registry.
  /// All variants will be registered too.
  /// Dependent types must be found in the registry.
  pub fn add_enumeration(&mut self, enumeration: Enumeration) -> Result<Uuid, RegistryError> {
    let enumeration = Rc::new(enumeration);
    let reg_ref = RegistryReference::Enumeration(enumeration.to_owned());
    let path = self.compute_path(&reg_ref)?;

    self.enumerations.push(enumeration.to_owned());
    let id = Uuid::new_v4();
    self.add_index(Selector::Id(id.to_owned()), reg_ref.to_owned())?;
    self.add_index(Selector::Path(path), reg_ref)?;
    for (variant_id, _) in &enumeration.variants {
      let variant_ref = RegistryReference::Variant(enumeration.to_owned(), variant_id.to_owned());
      let variant_path = self.compute_path(&variant_ref)?;
      self.add_index(Selector::Id(variant_id.to_owned()), variant_ref.to_owned())?;
      self.add_index(Selector::Path(variant_path), variant_ref.to_owned())?;
      todo!("check type");
    }
    Ok(id)
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
      RegistryReference::Module(entity) => self.find_id(&entity.parent).cloned(),
      RegistryReference::Root => None,
    };
    parent_ref.ok_or(RegistryError::UnknownParent {
      name: reg_ref.name().cloned().unwrap_or("<root>".to_string()),
    })
  }

  fn add_index(
    &mut self,
    selector: Selector,
    reg_ref: RegistryReference,
  ) -> Result<(), RegistryError> {
    match self.indexed.entry(selector.to_owned()) {
      Entry::Occupied(_) => return Err(RegistryError::DuplicateSelector { selector }),
      Entry::Vacant(entry) => entry.insert(reg_ref),
    };
    Ok(())
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
      RegistryReference::Module(entity) => &entity.name,
      RegistryReference::Root => return None,
    })
  }
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
}

impl std::error::Error for RegistryError {}

pub const ROOT_ID: Uuid = Uuid::from_bytes([
  0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00,
]);
