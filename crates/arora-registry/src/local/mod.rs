mod editable;
mod readable;
mod reg_ref;
pub(crate) use self::reg_ref::FrozenRegistryReference;
use self::reg_ref::LocalRegistryReference;
use crate::{
    EditableRegistry, EnumerationFrozen, FolderPublic, ModuleFrozen, RegistryError,
    StructureFrozen, TypeDefinitionFrozen,
};
use arora_types::module::declared::AroraModule;
use arora_types::record::Selector;
use arora_types::record::{FrozenReference, Resolver, UnfrozenReference};
use async_trait::async_trait;
use semver::Version;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    rc::Rc,
};
use uuid::Uuid;

/// A [`LocalRegistry`] supports the addition of [`StructureFrozen`], [`EnumerationFrozen`] and [`ModuleFrozen`]
/// on the fly. It provides a local index to look them up fast
/// by [`Uuid`] or by path ([`String`]).
/// It holds one record per identifier and version, and several versions of
/// a record side by side: adding the record it already holds under an
/// identifier and version keeps it — a structure's fields and an
/// enumeration's variants in the same order — and a different record there
/// is refused.
/// It can be used as a local cache of a remote registry accessed using the `remote` feature (semio-client).
/// It provides an absolute root available for any record,
/// with the identifier [`ROOT_ID`].
pub struct LocalRegistry {
    enumerations: Vec<Rc<EnumerationFrozen>>,
    structures: Vec<Rc<StructureFrozen>>,
    modules: Vec<Rc<ModuleFrozen>>,
    folders: Vec<Rc<FolderPublic>>, // folders cannot be frozen
    indexed: HashMap<Selector, BTreeMap<Version, FrozenRegistryReference>>,
    path_to_ids: HashMap<String, Uuid>,
}
unsafe impl Send for LocalRegistry {}
unsafe impl Sync for LocalRegistry {}

impl Default for LocalRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl LocalRegistry {
    pub fn new() -> Self {
        Self {
            folders: Vec::new(),
            enumerations: Vec::new(),
            structures: Vec::new(),
            modules: Vec::new(),
            indexed: HashMap::from([(
                Selector::Id(ROOT_ID.to_owned()),
                BTreeMap::from([(Version::new(0, 0, 0), FrozenRegistryReference::Root)]),
            )]),
            path_to_ids: HashMap::new(),
        }
    }

    pub fn find_frozen_by_id(&self, id: &Uuid) -> Option<&FrozenRegistryReference> {
        self.indexed
            .get(&Selector::Id(*id))
            .and_then(|version_index| get_latest_frozen(version_index))
    }

    /// Finds a record by its identifier.
    /// Searches first in the public index,
    /// then in the frozen index, and returns the latest version.
    pub fn find_latest(&self, id: &Uuid) -> Option<&dyn LocalRegistryReference> {
        self.find_frozen_by_id(id)
            .map(|r| r as &dyn LocalRegistryReference)
    }

    fn parent(
        &self,
        reg_ref: &dyn LocalRegistryReference,
    ) -> Result<&dyn LocalRegistryReference, RegistryError> {
        let new_unknown_parent_error = || RegistryError::UnknownParent {
            name: reg_ref.name().cloned().unwrap_or("<root>".to_string()),
        };
        let parent_id = reg_ref.parent().ok_or(new_unknown_parent_error())?;
        self.find_latest(parent_id)
            .ok_or(RegistryError::unknown_parent(
                parent_id.to_string().as_str(),
            ))
    }

    fn compute_path(&self, reg_ref: &dyn LocalRegistryReference) -> Result<String, RegistryError> {
        if reg_ref.is_root() {
            return Ok(String::new());
        }
        let record_name = reg_ref.name().expect("non-root record had no name");
        let parent = self.parent(reg_ref)?;
        let path = if parent.is_root() {
            record_name.to_owned()
        } else {
            format!("{}.{}", self.compute_path(parent)?, record_name)
        };
        Ok(path)
    }

    pub fn record_name_and_parent(&self, id: &Uuid) -> Option<(String, Option<Uuid>)> {
        let reg_ref = self.find_frozen_by_id(id)?;
        if reg_ref.is_root() {
            return None;
        }
        let name = reg_ref.name()?.clone();
        let parent = reg_ref.parent().cloned();
        Some((name, parent))
    }

    /// The module record held at exactly `id` and `version`. Answered from
    /// memory: no version requirement is matched and no path resolved.
    pub fn module_at(&self, id: &Uuid, version: &Version) -> Option<&ModuleFrozen> {
        match self.held_at(id, version)? {
            FrozenRegistryReference::Module { record, .. } => Some(record),
            _ => None,
        }
    }

    /// The structure or enumeration record held at exactly `id` and
    /// `version`. Answered from memory, like [`module_at`](Self::module_at).
    /// A primitive has no record.
    pub fn type_at(&self, id: &Uuid, version: &Version) -> Option<TypeDefinitionFrozen> {
        match self.held_at(id, version)? {
            FrozenRegistryReference::Enumeration { record, .. } => {
                Some(TypeDefinitionFrozen::Enumeration(record.as_ref().clone()))
            }
            FrozenRegistryReference::Structure { record, .. } => {
                Some(TypeDefinitionFrozen::Structure(record.as_ref().clone()))
            }
            _ => None,
        }
    }

    /// The records reachable from `reference` that the registry does not
    /// hold at exactly their version: the record itself when it is missing,
    /// else those it depends on, transitively — a module record's exports'
    /// types and declared dependencies, a structure's fields' and an
    /// enumeration's variants' types. A well-known type (a primitive, a uuid,
    /// a dynamic value) is not a record, so never missing. Empty when the
    /// registry holds the record and everything it depends on.
    pub fn missing_dependencies(&self, reference: &FrozenReference) -> Vec<FrozenReference> {
        self.missing_among(vec![reference.clone()])
    }

    /// The records reachable from `pending` the registry does not hold, in
    /// the order found.
    fn missing_among(&self, mut pending: Vec<FrozenReference>) -> Vec<FrozenReference> {
        let mut seen = HashSet::new();
        let mut missing = Vec::new();
        while let Some(reference) = pending.pop() {
            if arora_types::ty::WELL_KNOWN_IDS.contains(&reference.id)
                || !seen.insert(reference.clone())
            {
                continue;
            }
            let found: BTreeSet<FrozenReference> =
                match self.held_at(&reference.id, &reference.version.0) {
                    Some(FrozenRegistryReference::Module { record, .. }) => {
                        let mut references = HashSet::new();
                        record.dependencies(&mut references);
                        references.into_iter().cloned().collect()
                    }
                    Some(FrozenRegistryReference::Structure { record, .. }) => {
                        TypeDefinitionFrozen::Structure(record.as_ref().clone())
                            .references()
                            .into_iter()
                            .collect()
                    }
                    Some(FrozenRegistryReference::Enumeration { record, .. }) => {
                        TypeDefinitionFrozen::Enumeration(record.as_ref().clone())
                            .references()
                            .into_iter()
                            .collect()
                    }
                    Some(_) => BTreeSet::new(),
                    None => {
                        missing.push(reference);
                        continue;
                    }
                };
            // Reversed onto the stack, so dependencies are visited in order.
            pending.extend(found.into_iter().rev());
        }
        missing
    }

    /// Add the records of a module declared in Rust: its module record at
    /// `version` — the version its header carries — in the folder `parent`
    /// (the root, [`ROOT_ID`], or a folder the registry holds), and the
    /// records of the types its functions name, transitively
    /// ([`AroraModule::type_records`]): each in the folder its type declares,
    /// added with that folder's records, or in `parent` for a type declaring
    /// none. A record the registry already holds identically is kept, so
    /// modules sharing a type that declares its folder add together.
    ///
    /// Fails without adding the module record when it names a type that has
    /// no record of its own — one implementing `AroraType` without a record,
    /// or holding a fixed-length array, or naming such a type — naming each,
    /// and when a type record differs from the one the registry holds at its
    /// id and version: a type two modules share in different folders without
    /// declaring its own. The records it added before failing stay.
    pub async fn add_declared_module<M: AroraModule>(
        &mut self,
        version: Version,
        parent: Uuid,
    ) -> Result<(), RegistryError> {
        let records = M::type_records(parent);
        crate::local_yaml::add_folders(records.folders.into_iter().collect(), self).await?;
        for (reference, record) in records.types {
            let name = record.name();
            let (id, tag) = (reference.id, reference.version.0.clone());
            match record {
                TypeDefinitionFrozen::Structure(structure) => {
                    self.add_structure(id, tag, structure).await
                }
                TypeDefinitionFrozen::Enumeration(enumeration) => {
                    self.add_enumeration(id, tag, enumeration).await
                }
                TypeDefinitionFrozen::Primitive(_) => Ok(()),
            }
            .map_err(|error| match error {
                RegistryError::DuplicateVersion { .. }
                | RegistryError::DuplicateSelector { .. } => RegistryError::generic(&format!(
                    "module '{}' names type '{name}' ({reference}), whose record conflicts \
                         with one the registry holds ({error}): a type modules in different \
                         folders share declares its own folder",
                    M::record(parent).name,
                )),
                error => error,
            })?;
        }
        let module = M::record(parent);
        let mut references = HashSet::new();
        module.dependencies(&mut references);
        let missing = self.missing_among(references.into_iter().cloned().collect());
        if !missing.is_empty() {
            let missing: Vec<String> = missing.iter().map(ToString::to_string).collect();
            return Err(RegistryError::generic(&format!(
                "module '{}' ({}@{version}) names types without a record of their own: {} — a \
                 type implementing AroraType without a record, or holding a fixed-length array",
                module.name,
                M::id(),
                missing.join(", ")
            )));
        }
        self.add_module(M::id(), version, module).await
    }

    /// The record held at exactly `id` and `version`, if any.
    pub(crate) fn held_at(&self, id: &Uuid, version: &Version) -> Option<&FrozenRegistryReference> {
        self.indexed.get(&Selector::Id(*id))?.get(version)
    }

    /// Every record the registry holds — folders, enumerations, structures
    /// and modules, not the fields, variants and functions indexed under
    /// them — with its id and version.
    pub(crate) fn held(&self) -> impl Iterator<Item = (Uuid, &Version, &FrozenRegistryReference)> {
        self.indexed.iter().flat_map(|(selector, versions)| {
            let id = match selector {
                Selector::Id(id) => Some(*id),
                Selector::Path(_) => None,
            };
            versions.iter().filter_map(move |(version, reg_ref)| {
                let id = id?;
                match reg_ref {
                    FrozenRegistryReference::Folder { id: own, .. }
                    | FrozenRegistryReference::Enumeration { id: own, .. }
                    | FrozenRegistryReference::Structure { id: own, .. }
                    | FrozenRegistryReference::Module { id: own, .. }
                        if *own == id =>
                    {
                        Some((id, version, reg_ref))
                    }
                    _ => None,
                }
            })
        })
    }
}

#[async_trait]
impl Resolver for LocalRegistry {
    type Error = RegistryError;
    async fn resolve(&self, reference: &UnfrozenReference) -> Result<FrozenReference, Self::Error> {
        let selector = Selector::Id(reference.id.to_owned());
        // Primitives are intrinsic (fixed ids, no stored record or version);
        // resolve them directly, mirroring `get_type`, so modules don't need an
        // external primitive-records dump in the registry to be frozen.
        if crate::get_primitive(&selector).is_some() {
            return Ok(FrozenReference {
                id: reference.id,
                version: arora_types::record::Version(Version::new(0, 0, 0)),
            });
        }
        let version = self
            .indexed
            .get(&selector)
            .ok_or(RegistryError::no_such_record(&selector))?
            .iter()
            .rev()
            .find(|(version, _)| {
                if let Some(version_req) = &reference.version_req.0 {
                    version_req.matches(version)
                } else {
                    true
                }
            })
            .ok_or(RegistryError::NoSuchVersion {
                selector,
                version_req: reference.version_req.0.clone().unwrap_or_default(),
            })?
            .0
            .to_owned();
        Ok(FrozenReference {
            id: reference.id,
            version: arora_types::record::Version(version),
        })
    }
}

pub fn get_latest_frozen(
    version_index: &BTreeMap<Version, FrozenRegistryReference>,
) -> Option<&FrozenRegistryReference> {
    version_index.iter().last().map(|(_, r)| r)
}

pub use arora_types::record::folder::ROOT_ID;

#[cfg(test)]
mod tests {
    use super::{LocalRegistry, ROOT_ID};
    use crate::{EditableRegistry, EnumerationFrozen, ModuleFrozen};
    use arora_types::record::{
        enumeration::frozen::EnumerationVariant,
        module::frozen::{Export, ExportKind, Function},
        ty::{FrozenScalar, FrozenTy, Primitive, PrimitiveKind},
        FrozenReference,
    };
    use semver::{Version, VersionReq};
    use std::collections::{BTreeMap, HashMap};
    use uuid::Uuid;

    #[tokio::test]
    async fn add_status_enumeration_and_use_it_in_a_module() {
        let mut registry = LocalRegistry::new();

        let status = EnumerationFrozen {
            name: "Status".to_owned(),
            parent: ROOT_ID,
            variants: [(
                Uuid::new_v4(),
                EnumerationVariant {
                    name: "Ok".to_owned(),
                    ty: FrozenTy::Primitive(Primitive {
                        kind: arora_types::record::ty::PrimitiveKind::Unit,
                    }),
                },
            )]
            .into_iter()
            .collect(),
        };
        let status_version = Version::new(1, 0, 0);
        let enum_id = Uuid::new_v4();
        registry
            .add_enumeration(enum_id, status_version.to_owned(), status)
            .await
            .unwrap();

        let module = ModuleFrozen {
            parent: ROOT_ID,
            name: "node".to_owned(),
            exports: HashMap::from([(
                Uuid::new_v4(),
                Export {
                    name: "succeed".to_owned(),
                    kind: ExportKind::Function(Function {
                        parameters: HashMap::new(),
                        parameter_ordering: vec![],
                        return_ty: FrozenTy::FrozenScalar(FrozenScalar {
                            reference: FrozenReference {
                                id: enum_id,
                                version: status_version.into(),
                            },
                        }),
                    }),
                },
            )]),
            executable: None,
            dependencies: vec![],
        };
        registry
            .add_module(Uuid::new_v4(), Version::new(1, 0, 0), module)
            .await
            .unwrap();
    }

    #[test]
    pub fn versions() {
        let enumeration = EnumerationFrozen {
            name: "Status".to_string(),
            parent: ROOT_ID,
            variants: [
                (
                    Uuid::new_v4(),
                    EnumerationVariant {
                        name: "Success".to_string(),
                        ty: FrozenTy::Primitive(Primitive {
                            kind: PrimitiveKind::Unit,
                        }),
                    },
                ),
                (
                    Uuid::new_v4(),
                    EnumerationVariant {
                        name: "Failure".to_string(),
                        ty: FrozenTy::Primitive(Primitive {
                            kind: PrimitiveKind::Unit,
                        }),
                    },
                ),
                (
                    Uuid::new_v4(),
                    EnumerationVariant {
                        name: "Running".to_string(),
                        ty: FrozenTy::Primitive(Primitive {
                            kind: PrimitiveKind::Unit,
                        }),
                    },
                ),
            ]
            .into_iter()
            .collect(),
        };
        let version = Version::parse("1.0.0").unwrap();
        let mut enumerations_by_version = BTreeMap::new();
        enumerations_by_version.insert(version.clone(), enumeration.clone());
        let version_req = VersionReq::parse("=1").unwrap();
        let (matched_version, matched_enumeration) = enumerations_by_version
            .iter()
            .rev()
            .find(|(v, _)| version_req.matches(v))
            .unwrap();
        assert_eq!(*matched_version, version);
        assert_eq!(*matched_enumeration, enumeration);
    }
}
