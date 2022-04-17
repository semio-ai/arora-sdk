pub mod config;
pub mod local;
pub mod local_yaml;
pub mod remote;
pub mod remote_cached;
use arora_schema::{
  module::low::{Header, ModuleDefinition},
  ty::{
    low::Type, F32_ID, F64_ID, I16_ID, I32_ID, I64_ID, I8_ID, PRIMITIVE_TYPES, STRING_ID, U16_ID,
    U32_ID, U64_ID, U8_ID, UNIT_ID,
  },
};
use async_trait::async_trait;
use derive_more::Display;
use semio_client::common::{EntityType, Selector};
use semio_record::{
  enumeration::v0::Enumeration,
  module::v0::Module,
  record::{RecordDefn},
  structure::v0::Structure,
  ty::UnfrozenTy,
};
use semio_record::{
  folder::v0::Folder, organization::v0::Organization, ty::PrimitiveKind, user::v0::User,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use tokio::{
  fs::{read_to_string, File},
  io::AsyncReadExt,
};
use url::Url;
use uuid::Uuid;

const BASE_URL: &'static str = "https://raw.githubusercontent.com/semio-ai/arora-registry/master/";

pub struct Registry {
  base_uri: Url,
  type_id_cache: HashMap<String, Uuid>,
}

impl Registry {
  pub fn new() -> Self {
    Registry {
      base_uri: Url::parse(BASE_URL).unwrap(),
      type_id_cache: Registry::new_type_id_cache_with_primitives(),
    }
  }

  pub fn new_with_base_uri(base_uri: Url) -> Self {
    Registry {
      base_uri,
      type_id_cache: Registry::new_type_id_cache_with_primitives(),
    }
  }

  fn new_type_id_cache_with_primitives() -> HashMap<String, Uuid> {
    let mut type_id_cache = HashMap::new();
    PRIMITIVE_TYPES.iter().for_each(|(id, ty)| {
      type_id_cache.insert(ty.name.clone(), id.clone());
      ();
    });
    type_id_cache
  }

  async fn get_bytes(url: Url) -> anyhow::Result<Box<[u8]>> {
    if url.scheme() == "file" {
      let mut file = File::open(url.path()).await?;
      let mut data = Vec::new();
      file.read_to_end(&mut data).await?;
      Ok(data.into_boxed_slice())
    } else {
      Ok(
        reqwest::get(url)
          .await?
          .bytes()
          .await?
          .to_vec()
          .into_boxed_slice(),
      )
    }
  }

  async fn get_text(url: Url) -> anyhow::Result<String> {
    if url.scheme() == "file" {
      let path = if cfg!(windows) {
        &url.path()[1..]
      } else {
        url.path()
      };
      eprintln!("FILE URI {}", path);
      Ok(read_to_string(path).await?)
    } else {
      Ok(reqwest::get(url).await?.text().await?)
    }
  }

  pub async fn get_type(&self, id: &Uuid) -> anyhow::Result<Type> {
    let uri = self.base_uri.join(&format!("types/by-uuid/{id}.yaml"))?;
    let ret: Type = serde_yaml::from_str(&Self::get_text(uri.clone()).await?).map_err(|e| {
      RegistryError::ParsingError {
        message: format!("error parsing type info from {}: {}", uri, e),
      }
    })?;
    Ok(ret)
  }

  pub async fn lookup_type(&mut self, name: &str) -> anyhow::Result<Uuid> {
    if let Some(id) = self.type_id_cache.get(name) {
      return Ok(id.clone());
    }

    let uri = self.base_uri.join(&format!("types/by-name/{name}"))?;
    let text = Self::get_text(uri).await?;
    let id = Uuid::parse_str(&text)?;
    self.type_id_cache.insert(name.to_string(), id.clone());
    Ok(id)
  }

  pub async fn get_module_header(&self, id: &Uuid) -> anyhow::Result<Header> {
    let uri = self
      .base_uri
      .join(&format!("modules/by-uuid/{id}/header.yaml"))?;
    let text = Self::get_text(uri.clone()).await?;
    let header: Header = serde_yaml::from_str(&text).map_err(|e| RegistryError::ParsingError {
      message: format!("error parsing module info from {}: {}", uri, e),
    })?;
    Ok(header)
  }

  pub async fn get_module(&self, id: &Uuid) -> anyhow::Result<ModuleDefinition> {
    let header = self.get_module_header(id).await?;

    let uri = self
      .base_uri
      .join(&format!("modules/by-uuid/{id}/executable.bin"))?;
    let executable = Self::get_bytes(uri).await?;

    Ok(ModuleDefinition {
      schema_version: 0,
      header,
      executable,
    })
  }

  pub async fn lookup_module(&self, name: &str) -> anyhow::Result<Uuid> {
    let uri = self
      .base_uri
      .join(&format!("{BASE_URL}/modules/by-name/{name}"))?;
    Ok(Uuid::parse_str(&Self::get_text(uri).await?)?)
  }
}

#[async_trait(?Send)]
pub trait ReadableRegistry {
  /// Gets the definition of a type entity,
  /// i.e. of a primitive, a structure or an enumeration.
  /// Not to be confused with the [`type_of`] function,
  /// which retrieves the type of an entity.
  async fn get_type(&mut self, selector: &Selector) -> Result<TypeDefinition, RegistryError>;

  /// Gets the definition of a module.
  async fn get_module(&mut self, selector: &Selector) -> Result<ModulePublic, RegistryError>;

  /// Resolves the given selector into an identifier.
  async fn resolve_path(&mut self, path: &String) -> Result<Uuid, RegistryError>;

  /// Resolves the given identifier into a path.
  async fn resolve_id(&mut self, id: &Uuid) -> Result<String, RegistryError>;

  /// Resolves the type of entity identified by the given selector.
  /// Do not confuse with the [`get_type`] function,
  /// which returns type definitions.
  async fn type_of(&mut self, selector: &Selector) -> Result<EntityType, RegistryError>;
}

#[async_trait(?Send)]
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
  /// No such entity.
  #[display(fmt = "no such entity \"{}\"", selector)]
  NoSuchEntity { selector: Selector },

  /// Entity exists but is not a type.
  #[display(fmt = "entity \"{}\" exists but is not a type", selector)]
  NotAType { selector: Selector },

  /// Entity exists but is not a module.
  #[display(fmt = "entity \"{}\" exists but is not a module", selector)]
  NotAModule { selector: Selector },

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
