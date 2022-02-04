use std::{collections::{HashMap, HashSet}, fmt::Display};

use arora_schema::{
  module::low::{Header, ImportFunction, TypeRef, ExportSymbol},
};

use derive_more::Display;

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
      types: HashMap::new(),
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

#[derive(Debug)]
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

#[derive(Display, Debug)]
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

  #[test]
  pub fn new_index() -> Result<()> {
    Index::new();
    return Ok(());
  }
}
