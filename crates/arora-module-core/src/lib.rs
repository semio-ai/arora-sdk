use arora_schema::{
  module::low::{ExportSymbol, Header, ImportSymbol},
  ty::low::Type,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use bytes::{Buf, BufMut};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

#[derive(Debug, Serialize, Deserialize)]
pub enum Asset {
  Type(Type),
  ImportSymbol(ImportSymbol),
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
