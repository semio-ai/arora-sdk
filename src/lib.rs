pub mod module;
pub mod ty;

use derive_more::Display;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Display, Clone)]
#[display(fmt = "{}.{}.{}", major, minor, patch)]
pub struct SemanticVersion {
  major: u32,
  minor: u32,
  patch: u32,
}

mod tests {
  use std::str::FromStr;
  use uuid::Uuid;
  use crate::module::high::{ModuleDefinition, ExportSymbol};

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
  id: s32";
    let symbol: ExportSymbol = serde_yaml::from_str(function_string).unwrap();
    match symbol {
      ExportSymbol::Function(function) => assert!(function.name == "test"),
      _ => panic!("Parsed function export symbol not recognized"),
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
      id: s32
ret:
  kind: scalar
  id: s32";
    let symbol: ExportSymbol = serde_yaml::from_str(function_string).unwrap();
    match symbol {
      ExportSymbol::Function(function) => assert!(function.name == "test"),
      _ => panic!("Parsed function export symbol not recognized"),
    }
  }

  #[test]
  fn parse_simple_module() {
    let module_string = "\
id: 325c5e47-32db-4e23-a38f-7a2849647e0c
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
          id: s32
    ret:
      kind: scalar
      id: s32
imports: []
dependencies: []
executable_mime: application/wasm";

    let header: ModuleDefinition = serde_yaml::from_str(module_string).unwrap();
    assert!(header.name == "test-cpp");
  }
}
