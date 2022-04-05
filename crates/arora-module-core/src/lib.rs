use arora_registry::{ReadableRegistry, RegistryError, TypeDefinition};
use arora_schema::{
  module::{
    high::{ImportSymbol as HighImportSymbol, ModuleDefinition},
    low::{ExportSymbol, Header, ImportSymbol as LowImportSymbol},
  },
  ty::low::Type,
};
use bytes::{Buf, BufMut};
use derive_more::Display;
use semio_client::common::{EntityType, Selector};
use semio_record::{module::v0::public::Public as ModulePublic, record::UnfrozenReference};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{collections::HashSet, path::Path, str::FromStr};
use tokio::{
  fs::read_to_string,
  io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
};
use uuid::Uuid;

pub enum Asset2 {
  Type(TypeDefinition),
  Module(ModulePublic),
  Import(HighImportSymbol),
}

/// Analyzes a module from the path where it is written in the YAML format.
/// See [`analyze_module`].
pub async fn analyze_module_from_path<P: AsRef<Path>>(
  path: P,
  registry: &mut dyn ReadableRegistry,
) -> Result<Vec<Asset2>, ModuleDeclarationError> {
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
) -> Result<Vec<Asset2>, ModuleDeclarationError> {
  let mut assets = Vec::new();
  let mut deps_to_resolve = HashSet::<UnfrozenReference>::new();
  let mut module_deps = Vec::new();
  for HighImportSymbol::Function(import_function) in &module_definition.imports {
    let mod_selector = match Uuid::from_str(import_function.module.as_str()) {
      Ok(uuid) => Selector::Id(uuid),
      Err(_) => Selector::Path(import_function.module.to_owned()),
    }; // more modeselektor here https://soundcloud.com/modeselektor/wake-me-up-when-its-over
    let dep_module = registry
      .get_module(&mod_selector)
      .await
      .map_err(ModuleDeclarationError::RegistryError)?;
    for indirect_dep_ref in &dep_module.dependencies {
      deps_to_resolve.insert(indirect_dep_ref.clone());
    }
    module_deps.push(dep_module);
  }
  for dep_ref in deps_to_resolve {
    let selector = Selector::Id(dep_ref.id);
    let entity_type = registry
      .type_of(&selector)
      .await
      .map_err(ModuleDeclarationError::RegistryError)?;
    match entity_type {
      EntityType::Module => module_deps.push(
        registry
          .get_module(&selector)
          .await
          .map_err(ModuleDeclarationError::RegistryError)?,
      ),
      EntityType::Structure | EntityType::Enumeration => assets.push(Asset2::Type(
        registry
          .get_type(&selector)
          .await
          .map_err(ModuleDeclarationError::RegistryError)?,
      )),
      _ => (),
    }
  }
  assets.extend(module_deps.into_iter().map(Asset2::Module));
  assets.extend(module_definition.imports.into_iter().map(Asset2::Import));
  Ok(assets)
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Asset {
  Type(Type),
  ImportSymbol(LowImportSymbol),
  ExportSymbol(ExportSymbol),
  Header(Header),
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
  /// No such entity.
  RegistryError(RegistryError),

  /// IO error.
  IoError(std::io::Error),

  /// Serialization / deserialization error.
  YAMLError(serde_yaml::Error),

  /// For any other error.
  #[display(fmt = "error: {}", message)]
  Generic { message: String },
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
