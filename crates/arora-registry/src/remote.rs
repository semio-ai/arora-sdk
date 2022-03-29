use crate::{get_primitive, ModulePublic, ReadableRegistry, RegistryError, TypeDefinition};
use async_trait::async_trait;
use semio_client::{
  common::{EntityType, GetPublic, Selector, TypeOf},
  context::Context,
};
use uuid::Uuid;

pub struct RemoteRegistry {
  context: Context,
}

impl RemoteRegistry {
  /// Creates a new registry that will use the given context to communicate with the server.
  /// The context must be configured with a valid token.
  pub fn new(context: Context) -> Self {
    Self { context }
  }
}

#[async_trait(?Send)]
impl ReadableRegistry for RemoteRegistry {
  async fn get_type(&mut self, selector: &Selector) -> Result<TypeDefinition, RegistryError> {
    if let Some(primitive_kind) = get_primitive(selector) {
      return Ok(TypeDefinition::Primitive(primitive_kind));
    }
    let entity_type = semio_client::common::type_of(
      &self.context,
      TypeOf {
        selector: selector.clone(),
      },
    )
    .await
    .map_err(|e| RegistryError::RemoteError {
      message: format!("error getting type of {}: {}", selector.clone(), e),
    })?;
    let query = GetPublic {
      id: selector.clone(),
    };
    match entity_type {
      EntityType::Enumeration => {
        let enumeration = semio_client::enumeration::get_public(&self.context, query)
          .await
          .map_err(|e| RegistryError::RemoteError {
            message: format!("error getting enumeration {}: {}", selector.clone(), e),
          })?;
        Ok(TypeDefinition::Enumeration(enumeration))
      }
      EntityType::Structure => {
        let structure = semio_client::structure::get_public(&self.context, query)
          .await
          .map_err(|e| RegistryError::RemoteError {
            message: format!("error getting structure {}: {}", selector.clone(), e),
          })?;
        Ok(TypeDefinition::Structure(structure))
      }
      _ => Err(RegistryError::RemoteError {
        message: format!("{} is a {}, not a type", selector.clone(), entity_type),
      }),
    }
  }

  async fn get_module(&mut self, selector: &Selector) -> Result<ModulePublic, RegistryError> {
    let module = semio_client::module::get_public(
      &self.context,
      GetPublic {
        id: selector.clone(),
      },
    )
    .await
    .map_err(|e| RegistryError::RemoteError {
      message: format!("error getting module {}: {}", selector.clone(), e),
    })?;
    Ok(module)
  }

  async fn resolve(&mut self, selector: &Selector) -> Result<Uuid, RegistryError> {
    selector
      .resolve(&self.context)
      .await
      .map_err(|e| RegistryError::RemoteError {
        message: format!("error resolving {}: {}", selector.clone(), e),
      })
  }
}
