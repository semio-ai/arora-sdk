use crate::{
  get_primitive, EnumerationPublic, FolderPublic, ModulePublic, OrganizationPublic,
  ReadableRegistry, RegistryError, StructurePublic, TypeDefinition, UserPublic,
};
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

  async fn entity_type_of(&self, selector: &Selector) -> Result<EntityType, RegistryError> {
    semio_client::common::type_of(
      &self.context,
      TypeOf {
        selector: selector.clone(),
      },
    )
    .await
    .map_err(|e| RegistryError::RemoteError {
      message: format!("error getting type of {}: {}", selector.clone(), e),
    })
  }

  async fn get_enumeration(&self, selector: &Selector) -> Result<EnumerationPublic, RegistryError> {
    semio_client::enumeration::get_public(
      &self.context,
      GetPublic {
        id: selector.clone(),
      },
    )
    .await
    .map_err(|e| RegistryError::RemoteError {
      message: format!("error getting enumeration {}: {}", selector.clone(), e),
    })
  }

  async fn get_structure(&self, selector: &Selector) -> Result<StructurePublic, RegistryError> {
    semio_client::structure::get_public(
      &self.context,
      GetPublic {
        id: selector.clone(),
      },
    )
    .await
    .map_err(|e| RegistryError::RemoteError {
      message: format!("error getting structure {}: {}", selector.clone(), e),
    })
  }

  async fn get_user(&self, selector: &Selector) -> Result<UserPublic, RegistryError> {
    semio_client::user::get_public(
      &self.context,
      semio_client::user::GetPublic {
        selector: selector.clone(),
      },
    )
    .await
    .map_err(|e| RegistryError::RemoteError {
      message: format!("error getting user {}: {}", selector.clone(), e),
    })
  }

  async fn get_organization(
    &self,
    selector: &Selector,
  ) -> Result<OrganizationPublic, RegistryError> {
    semio_client::organization::get_public(
      &self.context,
      GetPublic {
        id: selector.clone(),
      },
    )
    .await
    .map_err(|e| RegistryError::RemoteError {
      message: format!("error getting organization {}: {}", selector.clone(), e),
    })
  }

  async fn get_folder(&self, selector: &Selector) -> Result<FolderPublic, RegistryError> {
    semio_client::folder::get_public(
      &self.context,
      GetPublic {
        id: selector.clone(),
      },
    )
    .await
    .map_err(|e| RegistryError::RemoteError {
      message: format!("error getting folder {}: {}", selector.clone(), e),
    })
  }
}

#[async_trait(?Send)]
impl ReadableRegistry for RemoteRegistry {
  async fn get_type(&mut self, selector: &Selector) -> Result<TypeDefinition, RegistryError> {
    if let Some(primitive_kind) = get_primitive(selector) {
      return Ok(TypeDefinition::Primitive(primitive_kind));
    }
    let entity_type = self.entity_type_of(selector).await?;

    match entity_type {
      EntityType::Enumeration => Ok(TypeDefinition::Enumeration(
        self.get_enumeration(selector).await?,
      )),
      EntityType::Structure => Ok(TypeDefinition::Structure(
        self.get_structure(selector).await?,
      )),
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

  async fn resolve_path(&mut self, path: &String) -> Result<Uuid, RegistryError> {
    let selector = Selector::Path(path.to_owned());
    selector
      .resolve(&self.context)
      .await
      .map_err(|e| RegistryError::RemoteError {
        message: format!("error resolving {}: {}", selector.clone(), e),
      })
  }

  async fn resolve_id(&mut self, id: &Uuid) -> Result<String, RegistryError> {
    let selector = Selector::Id(id.clone());
    if let Some(primitive_kind) = get_primitive(&selector) {
      return Ok(primitive_kind.to_string());
    }
    let entity_type = self.entity_type_of(&selector).await?;
    match entity_type {
      EntityType::Enumeration => {
        let enumeration = self.get_enumeration(&selector).await?;
        let parent_path = self.resolve_id(&enumeration.parent).await?;
        Ok(format!("{}.{}", parent_path, enumeration.name))
      }
      EntityType::Structure => {
        let structure = self.get_structure(&selector).await?;
        let parent_path = self.resolve_id(&structure.parent).await?;
        Ok(format!("{}.{}", parent_path, structure.name))
      }
      EntityType::User => {
        let user = self.get_user(&selector).await?;
        Ok(format!("{}", user.user_name))
      }
      EntityType::Organization => {
        let organization = self.get_organization(&selector).await?;
        Ok(format!("{}", organization.name))
      }
      EntityType::Folder => {
        let folder = self.get_folder(&selector).await?;
        let parent_path = self.resolve_id(&folder.parent).await?;
        Ok(format!("{}.{}", parent_path, folder.name))
      }
      EntityType::Module => {
        let module = self.get_module(&selector).await?;
        Ok(format!("{}", module.name))
      }
      _ => Err(RegistryError::RemoteError {
        message: format!("{} is of an unknown type", selector.clone()),
      }),
    }
  }
}
