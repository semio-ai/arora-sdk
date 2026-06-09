use super::{reg_ref::FrozenRegistryReference, LocalRegistry};
use crate::{get_primitive, ModuleFrozen, ReadableRegistry, RegistryError, TypeDefinitionFrozen};
use async_trait::async_trait;
use semio_client::common::{RecordType, Selector};
use semver::{Version, VersionReq};
use uuid::Uuid;

#[async_trait]
impl ReadableRegistry for LocalRegistry {
  async fn get_type(
    &mut self,
    selector: &Selector,
    tag: &VersionReq,
  ) -> Result<TypeDefinitionFrozen, RegistryError> {
    if let Some(primitive_kind) = get_primitive(selector) {
      return Ok(TypeDefinitionFrozen::Primitive(primitive_kind));
    }
    let (_, reg_ref) = self
      .indexed
      .get(selector)
      .ok_or(RegistryError::no_such_record(selector))?
      .iter()
      .rev()
      .find(|(version, _)| tag.matches(version))
      .ok_or(RegistryError::no_such_version(selector, tag))?;
    match reg_ref {
      FrozenRegistryReference::Enumeration { record, .. } => {
        Ok(TypeDefinitionFrozen::Enumeration(record.as_ref().clone()))
      }
      FrozenRegistryReference::Structure { record, .. } => {
        Ok(TypeDefinitionFrozen::Structure(record.as_ref().clone()))
      }
      _ => Err(RegistryError::NotAType {
        selector: selector.to_owned(),
      }),
    }
  }

  async fn get_module(
    &mut self,
    selector: &Selector,
    tag: &VersionReq,
  ) -> Result<ModuleFrozen, RegistryError> {
    let reg_ref = self
      .indexed
      .get(selector)
      .ok_or(RegistryError::no_such_record(selector))?
      .iter()
      .rev()
      .find(|(version, _)| tag.matches(version))
      .ok_or(RegistryError::no_such_version(selector, tag))?
      .1;
    match reg_ref {
      FrozenRegistryReference::Module { record, .. } => Ok(record.as_ref().clone()),
      _ => Err(RegistryError::not_a_module(selector)),
    }
  }

  async fn resolve_path(&self, path: &String) -> Result<Uuid, RegistryError> {
    Ok(
      *self
        .path_to_ids
        .get(path)
        .ok_or(RegistryError::NoSuchRecord {
          selector: Selector::Path(path.to_owned()),
        })?,
    )
  }

  async fn resolve_id(&self, id: &Uuid) -> Result<String, RegistryError> {
    let reg_ref = self
      .find_latest(id)
      .ok_or(RegistryError::no_such_record(&Selector::Id(id.to_owned())))?;
    self.compute_path(reg_ref)
  }

  async fn resolve_tag(
    &self,
    selector: &Selector,
    tag_req: &VersionReq,
  ) -> Result<Version, RegistryError> {
    Ok(
      self
        .indexed
        .get(selector)
        .ok_or(RegistryError::no_such_record(selector))?
        .iter()
        .rev()
        .find(|(version, _)| tag_req.matches(version))
        .ok_or(RegistryError::no_such_version(selector, tag_req))?
        .0
        .to_owned(),
    )
  }

  async fn type_of(&mut self, selector: &Selector) -> Result<RecordType, RegistryError> {
    self
      .indexed
      .get(selector)
      .and_then(|version_index| {
        version_index
          .iter()
          .last()
          .map(|(_, reg_ref)| match reg_ref {
            FrozenRegistryReference::Enumeration { .. } => RecordType::Enumeration,
            FrozenRegistryReference::Variant { .. } => RecordType::Unknown,
            FrozenRegistryReference::Structure { .. } => RecordType::Structure,
            FrozenRegistryReference::Field { .. } => RecordType::Unknown,
            FrozenRegistryReference::Module { .. } => RecordType::Module,
            FrozenRegistryReference::Function { .. } => RecordType::Unknown,
            FrozenRegistryReference::Folder { .. } => RecordType::Folder,
            FrozenRegistryReference::Root => RecordType::Unknown,
          })
      })
      .ok_or(RegistryError::NoSuchRecord {
        selector: selector.to_owned(),
      })
  }
}
