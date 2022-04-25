mod editable;
mod readable;
mod reg_ref;
use self::reg_ref::{FrozenRegistryReference, LatestRegistryReference, LocalRegistryReference};
use crate::{
  EnumerationFrozen, EnumerationPublic, FolderPublic, ModuleFrozen, ModulePublic, RegistryError,
  StructureFrozen, StructurePublic,
};
use async_trait::async_trait;
use semio_client::common::Selector;
use semio_record::record::{Freezer, FrozenReference, UnfrozenReference};
use semver::Version;
use std::{
  collections::{BTreeMap, HashMap},
  rc::Rc,
};
use uuid::Uuid;

/// A [`LocalRegistry`] supports the addition of [`Structure`], [`Enumeration`] and [`Module`]
/// on the fly. It provides a local index to look them up fast
/// by [`Uuid'] or by path ([`String`]).
/// It can be used as a local cache of a remote registry accessed using [`semio_client`].
/// It provides an absolute root available for any record,
/// with the identifier [`ROOT_ID`].
pub struct LocalRegistry {
  public_enumerations: Vec<Rc<EnumerationPublic>>,
  public_structures: Vec<Rc<StructurePublic>>,
  public_modules: Vec<Rc<ModulePublic>>,
  public_folders: Vec<Rc<FolderPublic>>,
  public_indexed: HashMap<Selector, LatestRegistryReference>,
  frozen_enumerations: Vec<Rc<EnumerationFrozen>>,
  frozen_structures: Vec<Rc<StructureFrozen>>,
  frozen_modules: Vec<Rc<ModuleFrozen>>,
  frozen_indexed: HashMap<Selector, BTreeMap<Version, FrozenRegistryReference>>,
  path_to_ids: HashMap<String, Uuid>,
}
unsafe impl Send for LocalRegistry {}
unsafe impl Sync for LocalRegistry {}

impl LocalRegistry {
  pub fn new() -> Self {
    Self {
      public_enumerations: Vec::new(),
      public_structures: Vec::new(),
      public_modules: Vec::new(),
      public_folders: Vec::new(),
      public_indexed: HashMap::from([(
        Selector::Id(ROOT_ID.to_owned()),
        LatestRegistryReference::Root,
      )]),
      frozen_enumerations: Vec::new(),
      frozen_structures: Vec::new(),
      frozen_modules: Vec::new(),
      frozen_indexed: HashMap::from([(
        Selector::Id(ROOT_ID.to_owned()),
        BTreeMap::from([(Version::new(0, 0, 0), FrozenRegistryReference::Root)]),
      )]),
      path_to_ids: HashMap::new(),
    }
  }

  fn find_public(&self, selector: &Selector) -> Option<&LatestRegistryReference> {
    self.public_indexed.get(selector)
  }

  fn find_public_by_id(&self, id: &Uuid) -> Option<&LatestRegistryReference> {
    self.find_public(&Selector::Id(id.to_owned()))
  }

  pub fn find_frozen_by_id(&self, id: &Uuid) -> Option<&FrozenRegistryReference> {
    self
      .frozen_indexed
      .get(&Selector::Id(id.to_owned()))
      .and_then(|version_index| get_latest_frozen(version_index))
  }

  /// Finds a record by its identifier.
  /// Searches first in the public index,
  /// then in the frozen index, and returns the latest version.
  pub fn find_latest(&self, id: &Uuid) -> Option<&dyn LocalRegistryReference> {
    self
      .find_public_by_id(id)
      .map(|r| r as &dyn LocalRegistryReference)
      .or_else(|| {
        self
          .find_frozen_by_id(id)
          .map(|r| r as &dyn LocalRegistryReference)
      })
  }

  fn parent(
    &self,
    reg_ref: &dyn LocalRegistryReference,
  ) -> Result<&dyn LocalRegistryReference, RegistryError> {
    let new_unknown_parent_error = || RegistryError::UnknownParent {
      name: reg_ref.name().cloned().unwrap_or("<root>".to_string()),
    };
    let parent_id = reg_ref.parent().ok_or(new_unknown_parent_error())?;
    self
      .find_latest(parent_id)
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
}

#[async_trait]
impl Freezer for LocalRegistry {
  type Error = RegistryError;
  async fn freeze(&self, reference: &UnfrozenReference) -> Result<FrozenReference, Self::Error> {
    let selector = Selector::Id(reference.id.to_owned());
    let version = self
      .frozen_indexed
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
      id: reference.id.clone(),
      version: semio_record::record::Version(version),
    })
  }
}

pub fn get_latest_frozen(
  version_index: &BTreeMap<Version, FrozenRegistryReference>,
) -> Option<&FrozenRegistryReference> {
  version_index.iter().last().map(|(_, r)| r)
}

pub const ROOT_ID: Uuid = Uuid::from_bytes([
  0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00,
]);

#[cfg(test)]
mod tests {
  use super::{LocalRegistry, ROOT_ID};
  use crate::{EditableRegistry, EnumerationFrozen, ModuleFrozen};
  use semio_record::{
    enumeration::v0::frozen::EnumerationVariant,
    module::v0::frozen::{Export, ExportKind, Function},
    record::FrozenReference,
    ty::{FrozenScalar, FrozenTy, Primitive, PrimitiveKind},
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
      variants: HashMap::from([(
        Uuid::new_v4(),
        EnumerationVariant {
          name: "Ok".to_owned(),
          ty: FrozenTy::Primitive(Primitive {
            kind: semio_record::ty::PrimitiveKind::Unit,
          }),
        },
      )]),
    };
    let status_version = Version::new(1, 0, 0);
    let enum_id = Uuid::new_v4();
    registry
      .add_enumeration_frozen(enum_id, status_version.to_owned(), status)
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
      .add_module_frozen(Uuid::new_v4(), Version::new(1, 0, 0), module)
      .await
      .unwrap();
  }

  #[test]
  pub fn versions() {
    let enumeration = EnumerationFrozen {
      name: "Status".to_string(),
      parent: ROOT_ID,
      variants: HashMap::from([
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
      ]),
    };
    let version = Version::parse("1.0.0").unwrap();
    let mut enumerations_by_version = BTreeMap::new();
    enumerations_by_version.insert(version.clone(), enumeration.clone());
    let version_req = VersionReq::parse("=1").unwrap();
    let (matched_version, matched_enumeration) = enumerations_by_version
      .iter()
      .rev()
      .find(|(v, _)| version_req.matches(*v))
      .unwrap();
    assert_eq!(*matched_version, version);
    assert_eq!(*matched_enumeration, enumeration);
  }
}
