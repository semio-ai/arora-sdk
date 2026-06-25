pub mod config;
pub mod local;
pub mod local_yaml;
pub mod remote;
pub mod remote_cached;
use arora_types::ty::{
    BOOLEAN_ID, F32_ID, F64_ID, I16_ID, I32_ID, I64_ID, I8_ID, STRING_ID, U16_ID, U32_ID, U64_ID,
    U8_ID, UNIT_ID,
};
use async_trait::async_trait;
use derive_more::Display;
use semio_client::common::{RecordType, Selector};
use semio_record::{
    enumeration::v0::Enumeration as EnumerationDefn, folder::v0::Folder as FolderDefn,
    module::v0::Module as ModuleDefn, organization::v0::Organization as OrganizationDefn,
    record::RecordDefn, structure::v0::Structure as StructureDefn, ty::FrozenTy, ty::PrimitiveKind,
    user::v0::User as UserDefn,
};
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use uuid::Uuid;

#[async_trait]
pub trait ReadableRegistry {
    /// Gets the definition of the latest version of a type matching the tag pattern.
    /// It can be a primitive, a structure or an enumeration.
    /// Not to be confused with the [`type_of`] function,
    /// which retrieves the type of an record.
    /// Gets the definition
    async fn get_type(
        &mut self,
        selector: &Selector,
        tag_req: &VersionReq,
    ) -> Result<TypeDefinitionFrozen, RegistryError>;

    /// Gets the definition of the latest version of a type matching the tag pattern.
    async fn get_module(
        &mut self,
        selector: &Selector,
        tag_req: &VersionReq,
    ) -> Result<ModuleFrozen, RegistryError>;

    /// Resolves the given selector into an identifier.
    async fn resolve_path(&self, path: &str) -> Result<Uuid, RegistryError>;

    /// Resolves the given identifier into a path.
    async fn resolve_id(&self, id: &Uuid) -> Result<String, RegistryError>;

    /// Resolves the latest tag of a record matching the requirement.
    async fn resolve_tag(
        &self,
        selector: &Selector,
        tag_req: &VersionReq,
    ) -> Result<Version, RegistryError>;

    /// Resolves the type of record identified by the given selector.
    /// Do not confuse with the [`get_type`] function,
    /// which returns type definitions.
    async fn type_of(&mut self, selector: &Selector) -> Result<RecordType, RegistryError>;
}

#[async_trait]
pub trait EditableRegistry {
    /// Adds an [`EnumerationFrozen`] to the registry.
    /// Its parent must be found in the registry.
    /// Its name must be unique under the given parent.
    /// Its identifier must be unique in the registry.
    /// All variants will be registered too.
    /// Returns the identifier under which the enumeration
    /// was registered.
    async fn add_enumeration(
        &mut self,
        id: Uuid,
        tag: Version,
        enumeration: EnumerationFrozen,
    ) -> Result<(), RegistryError>;

    /// Takes an unfrozen [`Enumeration`],
    /// freezes it with what is currently available in the registry,
    /// and adds it to the registry with the given identifier and version tag.
    async fn tag_enumeration(
        &mut self,
        id: Uuid,
        tag: Version,
        enumeration: Enumeration,
    ) -> Result<EnumerationFrozen, RegistryError>;

    /// Adds a [`StructureFrozen`] to the registry.
    /// Its parent must be found in the registry.
    /// Its name must be unique under the given parent.
    /// Its identifier must be unique in the registry.
    /// All fields will be registered too.
    /// Returns the identifier under which the structure
    /// was registered.
    async fn add_structure(
        &mut self,
        id: Uuid,
        tag: Version,
        structure: StructureFrozen,
    ) -> Result<(), RegistryError>;

    /// Takes an unfrozen [`Structure`],
    /// freezes it with what is currently available in the registry,
    /// and adds it to the registry with the given identifier and version tag.
    async fn tag_structure(
        &mut self,
        id: Uuid,
        tag: Version,
        structure: Structure,
    ) -> Result<StructureFrozen, RegistryError>;

    /// Adds a [`ModuleFrozen`] to the registry.
    /// Its parent must be found in the registry.
    /// Its name must be unique under the given parent.
    /// Its identifier must be unique in the registry.
    /// All fields will be registered too.
    /// Returns the identifier under which the module
    /// was registered.
    async fn add_module(
        &mut self,
        id: Uuid,
        tag: Version,
        module: ModuleFrozen,
    ) -> Result<(), RegistryError>;

    /// Takes an unfrozen [`Module`],
    /// freezes it with what is currently available in the registry,
    /// and adds it to the registry with the given identifier and version tag.
    async fn tag_module(
        &mut self,
        id: Uuid,
        tag: Version,
        module: Module,
    ) -> Result<ModuleFrozen, RegistryError>;

    /// Adds a folder to the registry, under the given identifier.
    async fn add_folder(&mut self, id: Uuid, folder: FolderPublic) -> Result<(), RegistryError>;
}

pub type Enumeration = <EnumerationDefn as RecordDefn>::Unfrozen;
pub type Structure = <StructureDefn as RecordDefn>::Unfrozen;
pub type Module = <ModuleDefn as RecordDefn>::Unfrozen;
pub type User = <UserDefn as RecordDefn>::Unfrozen;
pub type Organization = <OrganizationDefn as RecordDefn>::Unfrozen;
pub type Folder = <FolderDefn as RecordDefn>::Unfrozen;

pub type UserPublic = <UserDefn as RecordDefn>::Public;
pub type OrganizationPublic = <OrganizationDefn as RecordDefn>::Public;
pub type FolderPublic = <FolderDefn as RecordDefn>::Public;

pub type EnumerationFrozen = <EnumerationDefn as RecordDefn>::Frozen;
pub type StructureFrozen = <StructureDefn as RecordDefn>::Frozen;
pub type ModuleFrozen = <ModuleDefn as RecordDefn>::Frozen;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TypeDefinitionFrozen {
    Primitive(PrimitiveKind),
    Enumeration(EnumerationFrozen),
    Structure(StructureFrozen),
}

unsafe impl Send for TypeDefinitionFrozen {}

impl TypeDefinitionFrozen {
    pub fn name(&self) -> String {
        match self {
            Self::Primitive(primitive) => primitive.to_string(),
            Self::Enumeration(enumeration, ..) => enumeration.name.to_owned(),
            Self::Structure(structure, ..) => structure.name.to_owned(),
        }
    }

    pub fn direct_dependencies(&self) -> HashSet<Uuid> {
        let mut dependencies = HashSet::new();
        let mut maybe_insert = |ty: &FrozenTy| {
            match ty {
                FrozenTy::Primitive(_) => {}
                FrozenTy::FrozenScalar(scalar) => {
                    dependencies.insert(scalar.reference.id.to_owned());
                }
                FrozenTy::FrozenArray(array) => {
                    dependencies.insert(array.reference.id.to_owned());
                }
            };
        };
        match self {
            Self::Primitive(_) => {}
            Self::Enumeration(enumeration, ..) => enumeration
                .variants
                .iter()
                .for_each(|(_, variant)| maybe_insert(&variant.ty)),
            Self::Structure(structure, ..) => structure
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
    #[display("no such record \"{}\"", selector)]
    NoSuchRecord { selector: Selector },

    /// No such version of a record.
    #[display("no version matching record \"{}@{}\"", selector, version_req)]
    NoSuchVersion {
        selector: Selector,
        version_req: VersionReq,
    },

    /// Record exists but is not a type.
    #[display("record \"{}\" exists but is not a type", selector)]
    NotAType { selector: Selector },

    /// Record exists but is not a module.
    #[display("record \"{}\" exists but is not a module", selector)]
    NotAModule { selector: Selector },

    /// Record being inserted has a parent defined, but it is unknown locally.
    #[display("parent of record \"{}\" is unknown to the registry", name)]
    UnknownParent { name: String },

    /// The name or identifier of the record being added is already taken by another record.
    #[display(
        "added record's selector {} is already taken in the registry",
        selector
    )]
    DuplicateSelector { selector: Selector },

    /// The name or identifier of the record being added is already taken by another record.
    #[display(
        "record selector {}@{} is already present in the registry",
        selector,
        version
    )]
    DuplicateVersion {
        selector: Selector,
        version: Version,
    },

    /// Record being inserted uses a dependency that is not known locally.
    #[display("record depends on \"{}\", which is unknown to the registry", selector)]
    UnknownDependency { selector: Selector },

    /// Request to the registry failed because of an error in the remote service.
    #[display("remote service error: {}", message)]
    RemoteError { message: String },

    /// Error when parsing something, such as a behavior tree description.
    #[display("parsing error: {}", message)]
    ParsingError { message: String },

    /// For any other error.
    #[display("error: {}", message)]
    Generic { message: String },
}

impl std::error::Error for RegistryError {}

unsafe impl Send for RegistryError {}

impl RegistryError {
    pub fn no_such_record(selector: &Selector) -> Self {
        RegistryError::NoSuchRecord {
            selector: selector.to_owned(),
        }
    }

    pub fn no_such_version(selector: &Selector, version_req: &VersionReq) -> Self {
        RegistryError::NoSuchVersion {
            selector: selector.to_owned(),
            version_req: version_req.to_owned(),
        }
    }

    pub fn not_a_type(selector: &Selector) -> Self {
        RegistryError::NotAType {
            selector: selector.to_owned(),
        }
    }

    pub fn not_a_module(selector: &Selector) -> Self {
        RegistryError::NotAModule {
            selector: selector.to_owned(),
        }
    }

    pub fn unknown_parent(name: &str) -> Self {
        RegistryError::UnknownParent {
            name: name.to_owned(),
        }
    }

    pub fn duplicate_selector(selector: &Selector) -> Self {
        RegistryError::DuplicateSelector {
            selector: selector.to_owned(),
        }
    }

    pub fn duplicate_version(selector: &Selector, version: &Version) -> Self {
        RegistryError::DuplicateVersion {
            selector: selector.to_owned(),
            version: version.to_owned(),
        }
    }

    pub fn unknown_dependency(selector: &Selector) -> Self {
        RegistryError::UnknownDependency {
            selector: selector.to_owned(),
        }
    }

    pub fn remote_error<S: ToString>(message: S) -> Self {
        RegistryError::RemoteError {
            message: message.to_string(),
        }
    }

    pub fn parsing_error(message: &str) -> Self {
        RegistryError::ParsingError {
            message: message.to_owned(),
        }
    }

    pub fn generic(message: &str) -> Self {
        RegistryError::Generic {
            message: message.to_owned(),
        }
    }
}

pub fn get_primitive(selector: &Selector) -> Option<PrimitiveKind> {
    match selector {
        Selector::Id(id) => match id {
            id if *id == *UNIT_ID => Some(PrimitiveKind::Unit),
            id if *id == *BOOLEAN_ID => Some(PrimitiveKind::Boolean),
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
        Selector::Path(path) => path.parse().ok(),
    }
}
