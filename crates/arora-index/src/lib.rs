use std::{collections::{HashMap, HashSet}, fmt::Display};

use arora_schema::{
  module::low::{Header, ImportFunction, TypeRef, ExportSymbol}, ty::{PRIMITIVE_IDS, PRIMITIVE_LOW_TYPE_REFS},
};

use derive_more::{Display, Error};

use uuid::Uuid;

/// Local index of assets provided by modules.
pub struct Index {
  modules: HashMap<Uuid, Header>,
  types: HashMap<Uuid, TypeRef>,
  functions: HashMap<Uuid, ImportFunction>,
}

impl Index {
  pub fn new() -> Self {
    Self {
      modules: HashMap::new(),
      types: PRIMITIVE_LOW_TYPE_REFS.clone(),
      functions: HashMap::new(),
    }
  }

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

  pub fn find_function(&self, function_id: &Uuid) -> Result<&ImportFunction, UnresolvedFunctionError> {
    if let Some(function) = self.functions.get(function_id) {
      Ok(function)
    } else {
      Err(UnresolvedFunctionError { function_id: function_id.clone() })
    }
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
  pub fn add_a_module() -> Result<()> {
    let mut index = Index::new();
    let header: Header = serde_yaml::from_str(A_HEADER_YAML)?;
    index.add_module(&header)?;
    let function = index.find_function(&Uuid::from_str("07f5740c-ba4a-45af-8ec5-bedde5737e99")?)?;
    assert!(function.name == "test");
    Ok(())
  }

  pub const A_HEADER_YAML: &'static str = "\
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
