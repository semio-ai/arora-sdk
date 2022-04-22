use crate::{
  get_primitive, EnumerationPublic, FolderPublic, ModuleFrozen, ModulePublic, OrganizationPublic,
  ReadableRegistry, RegistryError, StructurePublic, TypeDefinitionFrozen, TypeDefinitionPublic,
  UserPublic,
};
use async_trait::async_trait;
use semio_client::{
  common::{type_of, GetPublic, RecordType, Selector, TaggedReq, TypeOf},
  context::Context,
};
use semio_record::record::{Freezer, FrozenReference, UnfrozenReference};
use semver::{Version, VersionReq};
use uuid::Uuid;

pub struct RemoteRegistry {
  context: Context,
}
unsafe impl Send for RemoteRegistry {}
unsafe impl Sync for RemoteRegistry {}

impl RemoteRegistry {
  /// Creates a new registry that will use the given context to communicate with the server.
  /// The context must be configured with a valid token.
  pub fn new(context: Context) -> Self {
    Self { context }
  }

  async fn record_type_of(&self, selector: &Selector) -> Result<RecordType, RegistryError> {
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

  async fn get_module_not_mut(&self, selector: &Selector) -> Result<ModulePublic, RegistryError> {
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

  pub async fn resolve_tag(
    &self,
    selector: &Selector,
    tag_req: &VersionReq,
  ) -> Result<Version, RegistryError> {
    let tags = semio_client::common::tags(
      &self.context,
      semio_client::common::Tags {
        selector: selector.to_owned(),
      },
    )
    .await
    .map_err(|e| RegistryError::RemoteError {
      message: format!("error listing tags for record {}: {}", selector.clone(), e),
    })?;
    tags
      .into_iter()
      .rev()
      .find(|tag| tag_req.matches(&tag.0))
      .ok_or_else(|| RegistryError::RemoteError {
        message: format!(
          "no tag matching {} found for record {}",
          tag_req,
          selector.clone()
        ),
      })
      .map(|tag| tag.0)
  }
}

#[async_trait]
impl ReadableRegistry for RemoteRegistry {
  async fn get_type(&mut self, selector: &Selector) -> Result<TypeDefinitionPublic, RegistryError> {
    if let Some(primitive_kind) = get_primitive(selector) {
      return Ok(TypeDefinitionPublic::Primitive(primitive_kind));
    }
    let record_type = self.record_type_of(selector).await?;

    match record_type {
      RecordType::Enumeration => Ok(TypeDefinitionPublic::Enumeration(
        self.get_enumeration(selector).await?,
      )),
      RecordType::Structure => Ok(TypeDefinitionPublic::Structure(
        self.get_structure(selector).await?,
      )),
      _ => Err(RegistryError::RemoteError {
        message: format!("{} is a {}, not a type", selector.clone(), record_type),
      }),
    }
  }

  async fn get_type_tagged(
    &mut self,
    selector: &Selector,
    tag: &VersionReq,
  ) -> Result<TypeDefinitionFrozen, RegistryError> {
    if let Some(primitive_kind) = get_primitive(selector) {
      return Ok(TypeDefinitionFrozen::Primitive(primitive_kind));
    }
    let record_type = self.record_type_of(selector).await?;

    match record_type {
      RecordType::Enumeration => Ok(TypeDefinitionFrozen::Enumeration(
        semio_client::enumeration::tagged_req(
          &self.context,
          TaggedReq {
            selector: selector.to_owned(),
            version_req: tag.to_string(),
          },
        )
        .await
        .map_err(|e| {
          RegistryError::remote_error(format!("error getting enumeration {}: {}", &selector, e))
        })?,
      )),
      RecordType::Structure => Ok(TypeDefinitionFrozen::Structure(
        semio_client::structure::tagged_req(
          &self.context,
          TaggedReq {
            selector: selector.to_owned(),
            version_req: tag.to_string(),
          },
        )
        .await
        .map_err(|e| {
          RegistryError::remote_error(format!("error getting enumeration {}: {}", &selector, e))
        })?,
      )),
      _ => Err(RegistryError::RemoteError {
        message: format!("{} is a {}, not a type", selector.clone(), record_type),
      }),
    }
  }

  async fn get_module(&mut self, selector: &Selector) -> Result<ModulePublic, RegistryError> {
    self.get_module_not_mut(selector).await
  }

  async fn get_module_tagged(
    &mut self,
    selector: &Selector,
    tag: &VersionReq,
  ) -> Result<ModuleFrozen, RegistryError> {
    let module = semio_client::module::tagged_req(
      &self.context,
      TaggedReq {
        selector: selector.to_owned(),
        version_req: tag.to_string(),
      },
    )
    .await
    .map_err(|e| RegistryError::RemoteError {
      message: format!("error getting module {}: {}", selector.clone(), e),
    })?;
    Ok(module)
  }

  async fn resolve_path(&self, path: &String) -> Result<Uuid, RegistryError> {
    let selector = Selector::Path(path.to_owned());
    selector
      .resolve(&self.context)
      .await
      .map_err(|e| RegistryError::RemoteError {
        message: format!("error resolving {}: {}", selector.clone(), e),
      })
  }

  async fn resolve_id(&self, id: &Uuid) -> Result<String, RegistryError> {
    let selector = Selector::Id(id.clone());
    if let Some(primitive_kind) = get_primitive(&selector) {
      return Ok(primitive_kind.to_string());
    }
    let record_type = self.record_type_of(&selector).await?;
    match record_type {
      RecordType::Enumeration => {
        let enumeration = self.get_enumeration(&selector).await?;
        let parent_path = self.resolve_id(&enumeration.parent).await?;
        Ok(format!("{}.{}", parent_path, enumeration.name))
      }
      RecordType::Structure => {
        let structure = self.get_structure(&selector).await?;
        let parent_path = self.resolve_id(&structure.parent).await?;
        Ok(format!("{}.{}", parent_path, structure.name))
      }
      RecordType::User => {
        let user = self.get_user(&selector).await?;
        Ok(format!("{}", user.user_name))
      }
      RecordType::Organization => {
        let organization = self.get_organization(&selector).await?;
        Ok(format!("{}", organization.name))
      }
      RecordType::Folder => {
        let folder = self.get_folder(&selector).await?;
        let parent_path = self.resolve_id(&folder.parent).await?;
        Ok(format!("{}.{}", parent_path, folder.name))
      }
      RecordType::Module => {
        let module = self.get_module_not_mut(&selector).await?;
        Ok(format!("{}", module.name))
      }
      _ => Err(RegistryError::RemoteError {
        message: format!("{} is of an unknown type", selector.clone()),
      }),
    }
  }

  async fn type_of(&mut self, selector: &Selector) -> Result<RecordType, RegistryError> {
    type_of(
      &self.context,
      TypeOf {
        selector: selector.clone(),
      },
    )
    .await
    .map_err(|err| RegistryError::Generic {
      message: format!("error getting type from remote: {}", err),
    })
  }
}

#[async_trait]
impl Freezer for RemoteRegistry {
  type Error = RegistryError;

  async fn freeze(&self, id: &UnfrozenReference) -> Result<FrozenReference, Self::Error> {
    let path = self.resolve_id(&id.id).await?;
    let selector = Selector::Path(format!("{}@{}", path, id.version_req));

    let tags = semio_client::common::tags(
      &self.context,
      semio_client::common::Tags {
        selector: selector.to_owned(),
      },
    )
    .await
    .map_err(|e| RegistryError::RemoteError {
      message: format!("error getting tags for {}: {}", selector.clone(), e),
    })?;
    let latest_tag = tags
      .last()
      .ok_or(RegistryError::NoSuchRecord { selector })?;
    Ok(FrozenReference {
      id: id.id.clone(),
      version: latest_tag.to_owned(),
    })
  }
}
