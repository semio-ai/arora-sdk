pub mod config;
pub mod local;
pub mod local_yaml;
pub mod remote;
pub mod remote_cached;
use arora_schema::ty::{
  F32_ID, F64_ID, I16_ID, I32_ID, I64_ID, I8_ID, STRING_ID, U16_ID, U32_ID, U64_ID, U8_ID, UNIT_ID,
};
use async_trait::async_trait;
use derive_more::Display;
use semio_client::common::{RecordType, Selector};
use semio_record::{
  enumeration::v0::Enumeration, module::v0::Module, record::RecordDefn, structure::v0::Structure,
  ty::UnfrozenTy,
};
use semio_record::{
  folder::v0::Folder, organization::v0::Organization, ty::PrimitiveKind, user::v0::User,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use uuid::Uuid;

#[async_trait]
pub trait ReadableRegistry {
  /// Gets the definition of a type record,
  /// i.e. of a primitive, a structure or an enumeration.
  /// Not to be confused with the [`type_of`] function,
  /// which retrieves the type of an record.
  async fn get_type(&mut self, selector: &Selector) -> Result<TypeDefinition, RegistryError>;

  /// Gets the definition of a module.
  async fn get_module(&mut self, selector: &Selector) -> Result<ModulePublic, RegistryError>;

  /// Resolves the given selector into an identifier.
  async fn resolve_path(&self, path: &String) -> Result<Uuid, RegistryError>;

  /// Resolves the given identifier into a path.
  async fn resolve_id(&self, id: &Uuid) -> Result<String, RegistryError>;

  /// Resolves the type of record identified by the given selector.
  /// Do not confuse with the [`get_type`] function,
  /// which returns type definitions.
  async fn type_of(&mut self, selector: &Selector) -> Result<RecordType, RegistryError>;
}

#[async_trait]
pub trait EditableRegistry {
  /// Adds an [`EnumerationPublic`] to the registry.
  /// Its parent must be found in the registry.
  /// Its name must be unique under the given parent.
  /// Its identifier must be unique in the registry.
  /// All variants will be registered too.
  /// Returns the identifier under which the enumeration
  /// was registered.
  async fn add_enumeration(
    &mut self,
    id: Uuid,
    enumeration: EnumerationPublic,
  ) -> Result<Uuid, RegistryError>;

  /// Adds a [`StructurePublic`] to the registry.
  /// Its parent must be found in the registry.
  /// Its name must be unique under the given parent.
  /// Its identifier must be unique in the registry.
  /// All fields will be registered too.
  /// Returns the identifier under which the structure
  /// was registered.
  async fn add_structure(
    &mut self,
    id: Uuid,
    structure: StructurePublic,
  ) -> Result<(), RegistryError>;

  /// Adds a [`ModulePublic`] to the registry.
  /// Its parent must be found in the registry.
  /// Its name must be unique under the given parent.
  /// Its identifier must be unique in the registry.
  /// All fields will be registered too.
  /// Returns the identifier under which the module
  /// was registered.
  async fn add_module(&mut self, id: Uuid, module: ModulePublic) -> Result<(), RegistryError>;

  /// Adds a folder to the registry, under the given identifier.
  async fn add_folder(&mut self, id: Uuid, folder: FolderPublic) -> Result<(), RegistryError>;
}

pub type EnumerationPublic = <Enumeration as RecordDefn>::Public;
pub type StructurePublic = <Structure as RecordDefn>::Public;
pub type ModulePublic = <Module as RecordDefn>::Public;
pub type UserPublic = <User as RecordDefn>::Public;
pub type OrganizationPublic = <Organization as RecordDefn>::Public;
pub type FolderPublic = <Folder as RecordDefn>::Public;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TypeDefinition {
  Primitive(PrimitiveKind),
  Enumeration(EnumerationPublic),
  Structure(StructurePublic),
}

unsafe impl Send for TypeDefinition {}

impl TypeDefinition {
  pub fn name(&self) -> String {
    match self {
      TypeDefinition::Primitive(primitive) => primitive.to_string(),
      TypeDefinition::Enumeration(enumeration) => enumeration.name.to_owned(),
      TypeDefinition::Structure(structure) => structure.name.to_owned(),
    }
  }

  pub fn direct_dependencies(&self) -> HashSet<Uuid> {
    let mut dependencies = HashSet::new();
    let mut maybe_insert = |ty: &UnfrozenTy| {
      match ty {
        UnfrozenTy::Primitive(_) => {}
        UnfrozenTy::UnfrozenScalar(scalar) => {
          dependencies.insert(scalar.reference.id.to_owned());
        }
        UnfrozenTy::UnfrozenArray(array) => {
          dependencies.insert(array.reference.id.to_owned());
        }
      };
    };
    match self {
      TypeDefinition::Primitive(_) => {}
      TypeDefinition::Enumeration(enumeration) => enumeration
        .variants
        .iter()
        .for_each(|(_, variant)| maybe_insert(&variant.ty)),
      TypeDefinition::Structure(structure) => structure
        .fields
        .iter()
        .for_each(|(_, field)| maybe_insert(&field.ty)),
    }
    dependencies
  }
}

#[derive(Display, Debug)]
pub enum RegistryError {
  /// No such record.
  #[display(fmt = "no such record \"{}\"", selector)]
  NoSuchRecord { selector: Selector },

  /// Record exists but is not a type.
  #[display(fmt = "record \"{}\" exists but is not a type", selector)]
  NotAType { selector: Selector },

  /// Record exists but is not a module.
  #[display(fmt = "record \"{}\" exists but is not a module", selector)]
  NotAModule { selector: Selector },

  /// Record being inserted has a parent defined, but it is unknown locally.
  #[display(fmt = "parent of record \"{}\" is unknown to the registry", name)]
  UnknownParent { name: String },

  /// The name or identifier of the record being added is already taken by another record.
  #[display(
    fmt = "added record's selector {} is already taken in the registry",
    selector
  )]
  DuplicateSelector { selector: Selector },

  /// Record being inserted uses a dependency that is not known locally.
  #[display(
    fmt = "record depends on \"{}\", which is unknown to the registry",
    selector
  )]
  UnknownDependency { selector: Selector },

  /// Request to the registry failed because of an error in the remote service.
  #[display(fmt = "remote service error: {}", message)]
  RemoteError { message: String },

  /// Error when parsing something, such as a behavior tree description.
  #[display(fmt = "parsing error: {}", message)]
  ParsingError { message: String },

  /// For any other error.
  #[display(fmt = "error: {}", message)]
  Generic { message: String },
}

impl std::error::Error for RegistryError {}

unsafe impl Send for RegistryError {}

pub fn get_primitive(selector: &Selector) -> Option<PrimitiveKind> {
  match selector {
    Selector::Id(id) => match id {
      id if *id == *UNIT_ID => Some(PrimitiveKind::Unit),
      id if *id == *U8_ID => Some(PrimitiveKind::U8),
      id if *id == *U16_ID => Some(PrimitiveKind::U16),
      id if *id == *U32_ID => Some(PrimitiveKind::U32),
      id if *id == *U64_ID => Some(PrimitiveKind::U64),
      id if *id == *I8_ID => Some(PrimitiveKind::I8),
      id if *id == *I16_ID => Some(PrimitiveKind::I16),
      id if *id == *I32_ID => Some(PrimitiveKind::I32),
      id if *id == *I64_ID => Some(PrimitiveKind::I64),
      id if *id == *F32_ID => Some(PrimitiveKind::F32),
      id if *id == *F64_ID => Some(PrimitiveKind::F64),
      id if *id == *STRING_ID => Some(PrimitiveKind::String),
      _ => None,
    },
    Selector::Path(path) => match path.parse() {
      Ok(primitive_kind) => Some(primitive_kind),
      Err(_) => None,
    },
  }
}
