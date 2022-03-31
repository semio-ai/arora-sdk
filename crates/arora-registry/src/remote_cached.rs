use crate::local::LocalRegistry;
use crate::remote::RemoteRegistry;
use crate::{EditableRegistry, ModulePublic, ReadableRegistry, RegistryError, TypeDefinition};
use async_trait::async_trait;
use semio_client::common::Selector;
use semio_client::context::Context;
use uuid::Uuid;

pub struct RemoteCachedRegistry {
  remote: RemoteRegistry,
  cache: LocalRegistry,
}

impl RemoteCachedRegistry {
  pub fn new(context: Context) -> Self {
    Self {
      remote: RemoteRegistry::new(context),
      cache: LocalRegistry::new(),
    }
  }
}

#[async_trait(?Send)]
impl ReadableRegistry for RemoteCachedRegistry {
  async fn get_type(&mut self, selector: &Selector) -> Result<TypeDefinition, RegistryError> {
    match self.cache.get_type(selector).await {
      Ok(ty) => Ok(ty),
      Err(RegistryError::NoSuchEntity { selector: _ }) => {
        let ty = self.remote.get_type(selector).await?;
        match &ty {
          TypeDefinition::Primitive(_) => {
            unreachable!("primitive type should have been found in cache");
          }
          TypeDefinition::Enumeration(enumeration) => {
            let id = self.resolve(selector).await?;
            self.cache.add_enumeration(id, enumeration.clone()).await?;
          }
          TypeDefinition::Structure(structure) => {
            let id = self.resolve(selector).await?;
            self.cache.add_structure(id, structure.clone()).await?;
          }
        }
        Ok(ty)
      }
      Err(e) => Err(e),
    }
  }

  async fn get_module(&mut self, selector: &Selector) -> Result<ModulePublic, RegistryError> {
    match self.cache.get_module(selector).await {
      Ok(module) => Ok(module),
      Err(RegistryError::NoSuchEntity { selector: _ }) => {
        let module = self.remote.get_module(selector).await?;
        let id = self.resolve(selector).await?;
        self.cache.add_module(id, module.clone()).await?;
        Ok(module)
      }
      Err(e) => Err(e),
    }
  }

  async fn resolve(&mut self, selector: &Selector) -> Result<Uuid, RegistryError> {
    let res = self.cache.resolve(selector).await;
    if res.is_ok() {
      return res;
    }
    self.remote.resolve(selector).await
  }
}
