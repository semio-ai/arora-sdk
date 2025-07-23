pub mod call;
pub mod keyvalue;
pub mod module;
pub mod ty;
pub mod value;

use derive_more::Display;
use semver::Version;
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use uuid::Uuid;

pub fn gen_bb_uuid_with_str(key: &String) -> uuid::Uuid {
  let node_ref = match Uuid::parse_str(&key) {
    Ok(uuid) => uuid,
    Err(_) => {
      // Generate a UUID based on the string key
      // Generate a deterministic UUID based on the string content
      let mut hasher = DefaultHasher::new();
      key.hash(&mut hasher);
      let hash = hasher.finish();

      // Create a byte array with the hash value
      let mut bytes = [0u8; 16];
      bytes[0..8].copy_from_slice(&hash.to_le_bytes());

      // Use part of the hash again for the second half
      let hash2 = hash.wrapping_mul(31);
      bytes[8..16].copy_from_slice(&hash2.to_le_bytes());

      // Set version to 4 and variant to RFC4122
      bytes[6] = (bytes[6] & 0x0F) | 0x40; // version 4
      bytes[8] = (bytes[8] & 0x3F) | 0x80; // variant

      Uuid::from_bytes(bytes)
    }
  };
  node_ref
}

pub fn gen_bb_uuid() -> Uuid {
  Uuid::new_v4()
}

#[derive(Serialize, Deserialize, Debug, Display, Clone)]
#[display("{}.{}.{}", major, minor, patch)]
pub struct SemanticVersion {
  pub major: u32,
  pub minor: u32,
  pub patch: u32,
}

impl Into<Version> for SemanticVersion {
  fn into(self) -> Version {
    Version::new(self.major.into(), self.minor.into(), self.patch.into())
  }
}

#[cfg(test)]
mod tests {
  use crate::module::high::{ExportSymbol, ModuleDefinition};
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
  fn parse_simple_function() {
    let function_string = "\
type: function
id: 07f5740c-ba4a-45af-8ec5-bedde5737e99
name: test
ret:
  kind: scalar
  id: i32";
    let symbol: ExportSymbol = serde_yaml::from_str(function_string).unwrap();
    match symbol {
      ExportSymbol::Function(function) => assert!(function.name == "test"),
    }
  }

  #[test]
  fn parse_function() {
    let function_string = "\
type: function
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
  id: i32";
    let symbol: ExportSymbol = serde_yaml::from_str(function_string).unwrap();
    match symbol {
      ExportSymbol::Function(function) => assert!(function.name == "test"),
    }
  }

  #[test]
  fn parse_simple_module() {
    let module_string = "\
id: 325c5e47-32db-4e23-a38f-7a2849647e0c
author: Semio
description: Test C++ module
license: Proprietary
name: test-cpp-2
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
    assert!(header.name == "test-cpp-2");
  }
}
