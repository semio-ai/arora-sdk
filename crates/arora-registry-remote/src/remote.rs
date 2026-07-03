use arora_registry::{
    get_primitive, EnumerationFrozen, FolderPublic, ModuleFrozen, ReadableRegistry, RegistryError,
    StructureFrozen, TypeDefinitionFrozen,
};
use arora_types::record::{FrozenReference, Resolver, UnfrozenReference};
use arora_types::record::{RecordType, Selector};
use async_trait::async_trait;
use semio_client::{
    common::{GetPublic, TaggedReq, TypeOf},
    context::Context,
};

/// Public user/organization records keep the Semio store's own vocabulary
/// (semio-record): they are store concepts, not arora ones.
pub type UserPublic = semio_record::user::v0::public::Public;
pub type OrganizationPublic = semio_record::organization::v0::public::Public;
use semver::{Version, VersionReq};
use uuid::Uuid;

/// The semio-client API speaks semio-record's types; arora speaks
/// `arora_types::record`. Their wire formats are identical by design (pinned
/// by arora-types' golden wire tests), so the conversion is a serde
/// round-trip through the shared format.
fn from_store<S, T>(record: S, what: &str) -> Result<T, RegistryError>
where
    S: serde::Serialize,
    T: serde::de::DeserializeOwned,
{
    serde_json::to_value(record)
        .and_then(serde_json::from_value)
        .map_err(|e| RegistryError::remote_error(format!("converting {what} record: {e}")))
}

/// Converts a public [`arora_types::record::Selector`] into the equivalent
/// `semio-client` selector accepted by the remote store API.
fn to_client_selector(selector: &Selector) -> semio_client::common::Selector {
    match selector {
        Selector::Id(id) => semio_client::common::Selector::Id(*id),
        Selector::Path(path) => semio_client::common::Selector::Path(path.clone()),
    }
}

/// Converts a `semio-client` record type returned by the remote store API
/// into the equivalent public [`arora_types::record::RecordType`].
fn from_client_record_type(record_type: semio_client::common::RecordType) -> RecordType {
    match record_type {
        semio_client::common::RecordType::User => RecordType::User,
        semio_client::common::RecordType::Folder => RecordType::Folder,
        semio_client::common::RecordType::Organization => RecordType::Organization,
        semio_client::common::RecordType::Module => RecordType::Module,
        semio_client::common::RecordType::Structure => RecordType::Structure,
        semio_client::common::RecordType::Enumeration => RecordType::Enumeration,
        semio_client::common::RecordType::Unknown => RecordType::Unknown,
    }
}

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
                selector: to_client_selector(selector),
            },
        )
        .await
        .map(from_client_record_type)
        .map_err(|e| RegistryError::RemoteError {
            message: format!("error getting type of {}: {}", selector.clone(), e),
        })
    }

    async fn get_enumeration(
        &self,
        selector: &Selector,
        tag: &VersionReq,
    ) -> Result<EnumerationFrozen, RegistryError> {
        semio_client::enumeration::tagged_req(
            &self.context,
            TaggedReq {
                selector: to_client_selector(selector),
                version_req: tag.to_string(),
            },
        )
        .await
        .map_err(|e| {
            RegistryError::remote_error(format!("error getting enumeration {}: {}", selector, e))
        })
        .and_then(|record| from_store(record, "enumeration"))
    }

    async fn get_structure(
        &self,
        selector: &Selector,
        tag: &VersionReq,
    ) -> Result<StructureFrozen, RegistryError> {
        semio_client::structure::tagged_req(
            &self.context,
            TaggedReq {
                selector: to_client_selector(selector),
                version_req: tag.to_string(),
            },
        )
        .await
        .map_err(|e| {
            RegistryError::remote_error(format!("error getting enumeration {}: {}", selector, e))
        })
        .and_then(|record| from_store(record, "structure"))
    }

    async fn get_user(&self, selector: &Selector) -> Result<UserPublic, RegistryError> {
        semio_client::user::get_public(
            &self.context,
            semio_client::user::GetPublic {
                selector: to_client_selector(selector),
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
                id: to_client_selector(selector),
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
                id: to_client_selector(selector),
            },
        )
        .await
        .map_err(|e| RegistryError::RemoteError {
            message: format!("error getting folder {}: {}", selector.clone(), e),
        })
        .and_then(|record| from_store(record, "folder"))
    }

    async fn get_module_not_mut(
        &self,
        selector: &Selector,
        tag: &VersionReq,
    ) -> Result<ModuleFrozen, RegistryError> {
        let module = semio_client::module::tagged_req(
            &self.context,
            TaggedReq {
                selector: to_client_selector(selector),
                version_req: tag.to_string(),
            },
        )
        .await
        .map_err(|e| RegistryError::RemoteError {
            message: format!("error getting module {}: {}", selector.clone(), e),
        })?;
        from_store(module, "module")
    }
}

#[async_trait]
impl ReadableRegistry for RemoteRegistry {
    async fn get_type(
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
                self.get_enumeration(selector, tag).await?,
            )),
            RecordType::Structure => Ok(TypeDefinitionFrozen::Structure(
                self.get_structure(selector, tag).await?,
            )),
            _ => Err(RegistryError::RemoteError {
                message: format!("{} is a {}, not a type", selector.clone(), record_type),
            }),
        }
    }

    async fn get_module(
        &mut self,
        selector: &Selector,
        tag: &VersionReq,
    ) -> Result<ModuleFrozen, RegistryError> {
        self.get_module_not_mut(selector, tag).await
    }

    async fn resolve_path(&self, path: &str) -> Result<Uuid, RegistryError> {
        let selector = Selector::Path(path.to_owned());
        to_client_selector(&selector)
            .resolve(&self.context)
            .await
            .map_err(|e| RegistryError::RemoteError {
                message: format!("error resolving {}: {}", selector.clone(), e),
            })
    }

    async fn resolve_id(&self, id: &Uuid) -> Result<String, RegistryError> {
        let selector = Selector::Id(*id);
        if let Some(primitive_kind) = get_primitive(&selector) {
            return Ok(primitive_kind.to_string());
        }
        let record_type = self.record_type_of(&selector).await?;
        match record_type {
            RecordType::Enumeration => {
                let enumeration = self.get_enumeration(&selector, &VersionReq::STAR).await?;
                let parent_path = self.resolve_id(&enumeration.parent).await?;
                Ok(format!("{}.{}", parent_path, enumeration.name))
            }
            RecordType::Structure => {
                let structure = self.get_structure(&selector, &VersionReq::STAR).await?;
                let parent_path = self.resolve_id(&structure.parent).await?;
                Ok(format!("{}.{}", parent_path, structure.name))
            }
            RecordType::User => {
                let user = self.get_user(&selector).await?;
                Ok(user.user_name.to_string())
            }
            RecordType::Organization => {
                let organization = self.get_organization(&selector).await?;
                Ok(organization.name.to_string())
            }
            RecordType::Folder => {
                let folder = self.get_folder(&selector).await?;
                let parent_path = self.resolve_id(&folder.parent).await?;
                Ok(format!("{}.{}", parent_path, folder.name))
            }
            RecordType::Module => {
                let module = self
                    .get_module_not_mut(&selector, &VersionReq::STAR)
                    .await?;
                Ok(module.name.to_string())
            }
            _ => Err(RegistryError::RemoteError {
                message: format!("{} is of an unknown type", selector.clone()),
            }),
        }
    }

    async fn resolve_tag(
        &self,
        selector: &Selector,
        tag_req: &VersionReq,
    ) -> Result<Version, RegistryError> {
        let tags = semio_client::common::tags(
            &self.context,
            semio_client::common::Tags {
                selector: to_client_selector(selector),
            },
        )
        .await
        .map_err(|e| RegistryError::RemoteError {
            message: format!("error listing tags for record {}: {}", selector.clone(), e),
        })?;
        tags.into_iter()
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

    async fn type_of(&mut self, selector: &Selector) -> Result<RecordType, RegistryError> {
        semio_client::common::type_of(
            &self.context,
            TypeOf {
                selector: to_client_selector(selector),
            },
        )
        .await
        .map(from_client_record_type)
        .map_err(|err| RegistryError::Generic {
            message: format!("error getting type from remote: {}", err),
        })
    }
}

#[async_trait]
impl Resolver for RemoteRegistry {
    type Error = RegistryError;

    async fn resolve(&self, id: &UnfrozenReference) -> Result<FrozenReference, Self::Error> {
        let path = self.resolve_id(&id.id).await?;
        let selector = Selector::Path(format!("{}@{}", path, id.version_req));

        let tags = semio_client::common::tags(
            &self.context,
            semio_client::common::Tags {
                selector: to_client_selector(&selector),
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
            id: id.id,
            version: arora_types::record::Version(latest_tag.0.to_owned()),
        })
    }
}
