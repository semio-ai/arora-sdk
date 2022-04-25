use crate::local::LocalRegistry;
use crate::remote::RemoteRegistry;
use crate::{
  EditableRegistry, Enumeration, EnumerationFrozen, FolderPublic, Module,
  ModuleFrozen, ReadableRegistry, RegistryError, Structure, StructureFrozen,
  TypeDefinitionFrozen,
};
use async_trait::async_trait;
use semio_client::common::{RecordType, Selector};
use semio_client::context::Context;
use semio_record::record::{Freezer, FrozenReference, UnfrozenReference};
use semver::{Version, VersionReq};
use uuid::Uuid;

pub struct RemoteCachedRegistry {
  remote: RemoteRegistry,
  cache: LocalRegistry,
}

unsafe impl Send for RemoteCachedRegistry {}

impl RemoteCachedRegistry {
  pub fn new(context: Context) -> Self {
    Self {
      remote: RemoteRegistry::new(context),
      cache: LocalRegistry::new(),
    }
  }

  pub async fn resolve_selector(&mut self, selector: &Selector) -> Result<Uuid, RegistryError> {
    match selector {
      Selector::Id(id) => Ok(id.clone()),
      Selector::Path(path) => self.resolve_path(path).await,
    }
  }
}

#[async_trait]
impl ReadableRegistry for RemoteCachedRegistry {
  async fn get_type_tagged(
    &mut self,
    selector: &Selector,
    tag_req: &VersionReq,
  ) -> Result<TypeDefinitionFrozen, RegistryError> {
    match self.cache.get_type_tagged(selector, tag_req).await {
      Ok(ty) => Ok(ty),
      Err(RegistryError::NoSuchRecord { selector: _ }) => {
        let ty = self.remote.get_type_tagged(selector, tag_req).await?;
        let tag = self.remote.resolve_tag(selector, tag_req).await?;
        match &ty {
          TypeDefinitionFrozen::Primitive(_) => {
            unreachable!("primitive type should have been found in cache");
          }
          TypeDefinitionFrozen::Enumeration(enumeration) => {
            let id = self.resolve_selector(selector).await?;
            self
              .cache
              .add_enumeration_frozen(id, tag.clone(), enumeration.clone())
              .await?;
          }
          TypeDefinitionFrozen::Structure(structure) => {
            let id = self.resolve_selector(selector).await?;
            self
              .cache
              .add_structure_frozen(id, tag.clone(), structure.clone())
              .await?;
          }
        }
        Ok(ty)
      }
      Err(e) => Err(e),
    }
  }

  async fn get_module_tagged(
    &mut self,
    selector: &Selector,
    tag_req: &VersionReq,
  ) -> Result<ModuleFrozen, RegistryError> {
    match self.cache.get_module_tagged(selector, tag_req).await {
      Ok(module) => Ok(module),
      Err(RegistryError::NoSuchRecord { selector: _ }) => {
        let module = self.remote.get_module_tagged(selector, tag_req).await?;
        let id = self.resolve_selector(selector).await?;
        let tag = self.remote.resolve_tag(selector, tag_req).await?;
        self
          .cache
          .add_module_frozen(id, tag.clone(), module.clone())
          .await?;
        Ok(module)
      }
      Err(e) => Err(e),
    }
  }

  async fn resolve_path(&self, path: &String) -> Result<Uuid, RegistryError> {
    let res = self.cache.resolve_path(path).await;
    if res.is_ok() {
      return res;
    }
    self.remote.resolve_path(path).await
  }

  async fn resolve_id(&self, id: &Uuid) -> Result<String, RegistryError> {
    let res = self.cache.resolve_id(id).await;
    if res.is_ok() {
      return res;
    }
    self.remote.resolve_id(id).await
  }

  async fn resolve_tag(
    &self,
    selector: &Selector,
    tag_req: &VersionReq,
  ) -> Result<Version, RegistryError> {
    let res = self.cache.resolve_tag(selector, tag_req).await;
    if res.is_ok() {
      return res;
    }
    self.remote.resolve_tag(selector, tag_req).await
  }

  async fn type_of(&mut self, selector: &Selector) -> Result<RecordType, RegistryError> {
    match self.cache.type_of(selector).await {
      Ok(ty) => Ok(ty),
      Err(RegistryError::NoSuchRecord { selector: _ }) => self.remote.type_of(selector).await,
      Err(err) => Err(err),
    }
  }
}

/// When an record is added, it is added to the local cache only.
#[async_trait]
impl EditableRegistry for RemoteCachedRegistry {
  async fn add_enumeration_frozen(
    &mut self,
    id: Uuid,
    tag: Version,
    enumeration: EnumerationFrozen,
  ) -> Result<(), RegistryError> {
    self
      .cache
      .add_enumeration_frozen(id, tag, enumeration)
      .await
  }

  async fn tag_enumeration(
    &mut self,
    id: Uuid,
    tag: Version,
    enumeration: Enumeration,
  ) -> Result<EnumerationFrozen, RegistryError> {
    self.cache.tag_enumeration(id, tag, enumeration).await
  }

  async fn add_structure_frozen(
    &mut self,
    id: Uuid,
    tag: Version,
    structure: StructureFrozen,
  ) -> Result<(), RegistryError> {
    self.cache.add_structure_frozen(id, tag, structure).await
  }

  async fn tag_structure(
    &mut self,
    id: Uuid,
    tag: Version,
    structure: Structure,
  ) -> Result<StructureFrozen, RegistryError> {
    self.cache.tag_structure(id, tag, structure).await
  }

  async fn add_module_frozen(
    &mut self,
    id: Uuid,
    tag: Version,
    module: ModuleFrozen,
  ) -> Result<(), RegistryError> {
    self.cache.add_module_frozen(id, tag, module).await
  }

  async fn tag_module(
    &mut self,
    id: Uuid,
    tag: Version,
    module: Module,
  ) -> Result<ModuleFrozen, RegistryError> {
    self.cache.tag_module(id, tag, module).await
  }
  async fn add_folder(&mut self, id: Uuid, folder: FolderPublic) -> Result<(), RegistryError> {
    self.cache.add_folder(id, folder).await
  }
}

#[async_trait]
impl Freezer for RemoteCachedRegistry {
  type Error = RegistryError;
  async fn freeze(&self, reference: &UnfrozenReference) -> Result<FrozenReference, Self::Error> {
    match self.cache.freeze(reference).await {
      Ok(frozen) => Ok(frozen),
      Err(RegistryError::NoSuchRecord { .. }) | Err(RegistryError::NoSuchVersion { .. }) => {
        Ok(self.remote.freeze(reference).await?)
      }
      Err(err) => Err(err),
    }
  }
}
