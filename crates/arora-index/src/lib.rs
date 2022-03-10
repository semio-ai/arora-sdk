use std::{collections::{HashMap, HashSet}, fmt::Display};

use arora_schema::{
  module::low::{Header, ImportFunction, ExportSymbol},
  ty::{PRIMITIVE_TYPES, low::Type}, value::Value,
};

use derive_more::{Display, Error};

use uuid::Uuid;

/// Local index of assets provided by modules.
pub struct Index {
  pub modules: HashMap<Uuid, Header>,
  pub types: HashMap<Uuid, Type>,
  pub functions: HashMap<Uuid, ImportFunction>,
}

impl Index {
  pub fn new() -> Self {
    Self {
      modules: HashMap::new(),
      types: PRIMITIVE_TYPES.clone(),
      functions: HashMap::new(),
    }
  }

  /// Add a module, and its exported functions into the index.
  /// Dependent types must have been added to the module beforehand.
  pub fn add_module(&mut self, header: &Header) -> Result<(), UnresolvedTypesError> {
    self.modules.insert(header.id.clone(), header.clone());
    let mut unresolved_types = HashSet::<Uuid>::new();
    header.exports.iter().for_each(|e| {
      match e {
        ExportSymbol::Function(function) => {
          let types = function.type_dependencies();
          types.iter()
            .filter(|type_id| !self.types.contains_key(type_id))
            .for_each(|type_id| {
              unresolved_types.insert(type_id.clone());
              ()
            });

          let function = ImportFunction {
            module: header.id.clone(),
            id: function.id.clone(),
            name: function.name.clone(),
            parameters: function.parameters.clone(),
            ret: function.ret.clone(),
          };
          self.functions.insert(e.id().clone(), function);
        }
      }
    });
    
    if unresolved_types.is_empty() {
      Ok(())      
    } else {
      Err(UnresolvedTypesError {
        type_ids: unresolved_types.iter().cloned().collect::<Vec<Uuid>>()
      })
    }
  }

  pub fn add_type(&mut self, ty: Type) {
    self.types.insert(ty.id.clone(), ty);
    ()
  }

  pub fn find_type(&self, type_id: &Uuid) -> Result<&Type, UnresolvedTypesError> {
    self.types.get(type_id)
      .ok_or(UnresolvedTypesError { type_ids: vec![type_id.clone()] })
  }

  pub fn find_function(&self, function_id: &Uuid) -> Result<&ImportFunction, UnresolvedFunctionError> {
    self.functions.get(function_id)
      .ok_or(UnresolvedFunctionError { function_id: function_id.clone() })
  }
}

#[derive(Debug, Error)]
pub struct UnresolvedTypesError {
  type_ids: Vec<Uuid>,
}

impl Display for UnresolvedTypesError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(
          format_args!("following types are referenced but are unknown {:?}", self.type_ids)
        )
    }
}

#[derive(Display, Debug, Error)]
pub struct UnresolvedFunctionError {
  #[display(fmt = "no function found with id {}", function_id)]
  function_id: Uuid,
}

// Tests.
//=====================================================================
#[cfg(test)]
mod tests {
  use super::*;
  use anyhow::Result;
  use std::str::FromStr;

  #[test]
  pub fn new_index() -> Result<()> {
    Index::new();
    return Ok(());
  }

  #[test]
  pub fn add_type() -> Result<()> {
    let mut index = Index::new();
    let status_type: Type = serde_yaml::from_str(STATUS_ENUM_YAML)?;
    index.add_type(status_type);
    return Ok(());
  }

  #[test]
  pub fn add_two_types() -> Result<()> {
    let index = new_index_with_types()?;
    let status_type = index.find_type(&Uuid::from_str("325a5767-e344-4532-860e-0749bcf2e428")?)?;
    debug_assert_eq!("Status", status_type.name);
    let structure_type = index.find_type(&Uuid::from_str("7f9aedf8-dbde-4020-b5f4-c28a6635ae7c")?)?;
    debug_assert_eq!("TestStructure1", structure_type.name);
    return Ok(());
  }

  #[test]
  pub fn add_two_types_wrong_order() -> Result<()> {
    let mut index = Index::new();
    let status_type: Type = serde_yaml::from_str(STATUS_ENUM_YAML)?;
    let structure_type: Type = serde_yaml::from_str(SOME_STRUCTURE_YAML)?;
    index.add_type(structure_type);
    index.add_type(status_type);
    return Ok(());
  }

  fn new_index_with_types() -> Result<Index> {
    let mut index = Index::new();
    let status_type: Type = serde_yaml::from_str(STATUS_ENUM_YAML)?;
    index.add_type(status_type);
    let structure_type: Type = serde_yaml::from_str(SOME_STRUCTURE_YAML)?;
    index.add_type(structure_type);
    Ok(index)
  }

  #[test]
  pub fn add_a_module() -> Result<()> {
    let index = new_index_with_some_module()?;
    let function = index.find_function(&Uuid::from_str("07f5740c-ba4a-45af-8ec5-bedde5737e99")?)?;
    debug_assert_eq!(function.name, "test");
    Ok(())
  }

  fn new_index_with_some_module() -> Result<Index> {
    let mut index = new_index_with_types()?;
    let header: Header = serde_yaml::from_str(SOME_HEADER_YAML)?;
    index.add_module(&header)?;
    Ok(index)
  }

  pub const STATUS_ENUM_YAML: &'static str = "\
---
name: Status
id: 325a5767-e344-4532-860e-0749bcf2e428
description: Behavior Tree status value (success, failure, running)
kind:
  type: enumeration
  values:
    766e9e9a-446d-4e46-83e6-14b7ca101169:
      name: Success
      type:
        kind: scalar
        id: 00000000-0000-0000-0000-000000000000
    2468f46c-bb60-425c-9a4d-9ad326ccc7e2:
      name: Failure
      type:
        kind: scalar
        id: 00000000-0000-0000-0000-000000000000
    acd79ec6-0c44-401a-82f8-5da5422d3eec:
      name: Running
      type:
        kind: scalar
        id: 00000000-0000-0000-0000-000000000000
";

  pub const SOME_STRUCTURE_YAML: &'static str = "\
---
name: TestStructure1
id: 7f9aedf8-dbde-4020-b5f4-c28a6635ae7c
description: Test Structure 1
kind:
  type: structure
  fields:
    7d94a956-e50d-4cc4-9714-f62e1f9b134e:
      name: status
      type:
        kind: array
        id: 325a5767-e344-4532-860e-0749bcf2e428
    5ffa9104-1e5c-4026-943f-8db38bd34563:
      name: integer_array
      type:
        kind: array
        id: 00000000-0000-0000-0000-000000000004
";

  pub const SOME_HEADER_YAML: &'static str = "\
---
id: 325c5e47-32db-4e23-a38f-7a2849647e0c
name: test-cpp
author: Semio
description: Test C++ module
license: Proprietary
version:
  major: 0
  minor: 1
  patch: 0
executor:
  name: wasm
  min_version: ~
  max_version: ~
exports:
  - type: function
    id: 07f5740c-ba4a-45af-8ec5-bedde5737e99
    name: test
    parameters:
      - id: b41899c3-66dc-40d4-ab61-d1ccf5231c88
        name: a
        type:
          kind: scalar
          id: 325a5767-e344-4532-860e-0749bcf2e428
        mutable: false
      - id: 63086e48-804f-403a-8862-3358ddedc08d
        name: b
        type:
          kind: scalar
          id: 7f9aedf8-dbde-4020-b5f4-c28a6635ae7c
        mutable: false
    ret:
      kind: scalar
      id: 00000000-0000-0000-0000-000000000004
imports:
  - type: function
    module: 23a75559-025c-4be8-8b48-0784e30dc9ec
    id: b213a552-77ad-465a-a26d-352e8eccfd63
    name: test_2
    parameters:
      - id: 55dbec70-1c3a-433e-a6e6-27446b7f065e
        name: a
        type:
          kind: scalar
          id: 00000000-0000-0000-0000-000000000008
        mutable: false
      - id: abf9ca4e-e03f-431a-a32b-4911f809c399
        name: b
        type:
          kind: scalar
          id: 00000000-0000-0000-0000-000000000008
        mutable: false
    ret:
      kind: scalar
      id: 325a5767-e344-4532-860e-0749bcf2e428
executable_mime: application/wasm
"; 
}
