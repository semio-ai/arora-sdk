pub mod header;
pub mod resolve;
use arora_registry::{ReadableRegistry, RegistryError, TypeDefinition};
use arora_schema::module::high::ModuleDefinition;
use arora_vfs::VfsError;
use bytes::{Buf, BufMut};
use derive_more::Display;
use resolve::resolve_high_module;
use semio_client::common::{RecordType, Selector};
use semio_record::module::v0::{public::Public as ModulePublic, unfrozen::Export};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::path::Path;
use tokio::{
  fs::read_to_string,
  io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
};
use uuid::Uuid;

/// Analyzes a module from the path where it is written in the YAML format.
/// See [`analyze_module`].
pub async fn analyze_module_from_path<P: AsRef<Path>>(
  path: P,
  registry: &mut dyn ReadableRegistry,
) -> Result<Vec<ModuleAsset>, ModuleDeclarationError> {
  let module_yaml = read_to_string(path)
    .await
    .map_err(ModuleDeclarationError::IoError)?;
  let module_definition: ModuleDefinition =
    serde_yaml::from_str(&module_yaml).map_err(ModuleDeclarationError::YAMLError)?;
  analyze_module(module_definition, registry).await
}

/// Analyzes a module by reading its header and
/// resolves its dependencies with the help of the provided registry.
/// Produces a list of assets that can be used for code generation.
/// First, the types, then the modules, then the imports.
pub async fn analyze_module(
  module_definition: ModuleDefinition,
  registry: &mut dyn ReadableRegistry,
) -> Result<Vec<ModuleAsset>, ModuleDeclarationError> {
  let module_id = module_definition.id.clone();

  // Resolve the module contents into a description compatible with the registry.
  // It already includes the dependencies (internal and external) as references.
  let resolved_module = resolve_high_module(module_definition, registry).await?;

  // Collect first the actual types behind the references.
  let mut assets = Vec::new();
  for dep_ref in &resolved_module.module.dependencies {
    let selector = Selector::Id(dep_ref.id);
    let record_type = registry
      .type_of(&selector)
      .await
      .map_err(ModuleDeclarationError::RegistryError)?;
    match record_type {
      RecordType::Structure | RecordType::Enumeration => assets.push(ModuleAsset::Type(
        dep_ref.id.to_owned(),
        registry
          .get_type(&selector)
          .await
          .map_err(ModuleDeclarationError::RegistryError)?,
      )),
      _ => (),
    }
  }

  // Then publish imports, and then this module.
  assets.extend(resolved_module.imports.into_iter().map(ModuleAsset::Import));
  assets.push(ModuleAsset::Module(module_id, resolved_module.module));
  Ok(assets)
}

/// Assets are records provided or referred to by a module.
#[derive(Debug, Serialize, Deserialize)]
pub enum ModuleAsset {
  /// Type, including its identifier.
  Type(Uuid, TypeDefinition),
  /// Imported symbol, including the identifier of its origin module.
  Import(ImportAsset),
  /// Module, including its identifier.
  Module(Uuid, ModulePublic),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportAsset {
  pub module_id: Uuid,
  pub module_name: String,
  pub id: Uuid,
  pub import: Export,
}

pub struct Writer<'a, W: AsyncWrite + Unpin> {
  writer: &'a mut W,
}

impl<'a, W: AsyncWrite + Unpin> Writer<'a, W> {
  pub fn new(writer: &'a mut W) -> Self {
    Self { writer }
  }

  pub async fn write<T: Serialize>(&mut self, value: T) -> tokio::io::Result<()> {
    let mut size = [0u8; 4];
    let serialized = serde_json::to_string(&value).unwrap();
    (&mut size[..]).put_u32(serialized.len() as u32);
    self.writer.write_all(&size).await?;
    self.writer.write_all(serialized.as_bytes()).await?;
    Ok(())
  }

  pub async fn end(self) -> tokio::io::Result<()> {
    let mut size = [0u8; 4];
    (&mut size[..]).put_u32(0);
    self.writer.write_all(&size).await?;
    Ok(())
  }
}

pub struct Reader<'a, R: AsyncRead + Unpin> {
  reader: &'a mut R,
}

impl<'a, R: AsyncRead + Unpin> Reader<'a, R> {
  pub fn new(reader: &'a mut R) -> Self {
    Self { reader }
  }

  pub async fn read<T: DeserializeOwned>(&mut self) -> tokio::io::Result<Option<T>> {
    let mut size = [0u8; 4];
    self.reader.read_exact(&mut size).await?;
    let size = (&size[..]).get_u32() as usize;
    if size == 0 {
      return Ok(None);
    }

    let mut buf = vec![0u8; size];
    self.reader.read_exact(&mut buf).await?;
    let value: T = serde_json::from_slice(&buf).unwrap();
    Ok(Some(value))
  }
}

#[derive(Display, Debug)]
pub enum ModuleDeclarationError {
  /// Record is not known to the registry or registry is not available.
  RegistryError(RegistryError),

  /// IO error.
  IoError(std::io::Error),

  /// Serialization / deserialization error.
  YAMLError(serde_yaml::Error),

  /// Virtual file system error.
  VfsError(VfsError),

  /// For any other error.
  #[display(fmt = "error: {}", _0)]
  Generic(String),
}

impl std::error::Error for ModuleDeclarationError {}

#[cfg(test)]
mod tests {
  use arora_schema::module::high::ModuleDefinition;
  use std::str::FromStr;
  use uuid::Uuid;

  #[test]
  fn parse_uuid() {
    let uuid_string = "b41899c3-66dc-40d4-ab61-d1ccf5231c88";
    let expected = Uuid::from_str(uuid_string).unwrap();
    let actual: Uuid = serde_yaml::from_str(uuid_string).unwrap();
    assert!(actual == expected);
  }

  #[test]
  fn load_simple_module() {
    let module_string = "id: 325c5e47-32db-4e23-a38f-7a2849647e0c
author: Semio
description: Test C++ module
license: Proprietary
name: test-cpp
version:
  major: 0
  minor: 1
  patch: 0
executor:
  name: wasm
exports:
  - type: function
    id: 07f5740c-ba4a-45af-8ec5-bedde5737e99
    name: test
    parameters:
      - id: b41899c3-66dc-40d4-ab61-d1ccf5231c88
        name: a
        type:
          kind: scalar
          id: Status
      - id: 63086e48-804f-403a-8862-3358ddedc08d
        name: b
        type:
          kind: scalar
          id: i32
    ret:
      kind: scalar
      id: i32
imports: []
dependencies: []
executable_mime: application/wasm";

    let header: ModuleDefinition = serde_yaml::from_str(module_string).unwrap();
    assert!(header.name == "test-cpp");
  }
}
