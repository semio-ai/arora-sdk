use std::collections::HashSet;

use crate::SemanticVersion;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Executor {
  /// The executor name (e.g., WebAssembly, Python, Javascript, etc.)
  pub name: String,
  pub min_version: Option<SemanticVersion>,
  pub max_version: Option<SemanticVersion>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum TypeRef {
  Scalar {
    id: Uuid
  },
  Array {
    id: Uuid
  },
  Map {
    key_id: Uuid,
    value_id: Uuid
  },
}

impl TypeRef {
  pub fn type_dependencies(&self) -> HashSet<Uuid> {
    let mut deps = HashSet::new();
    match self {
      TypeRef::Scalar { id } => deps.insert(*id),
      TypeRef::Array {id } => deps.insert(*id),
      TypeRef::Map { key_id, value_id } => {
        deps.insert(*key_id);
        deps.insert(*value_id)
      }
    };
    deps
  }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Parameter {
  /// ID
  pub id: Uuid,
  /// Name
  pub name: String,
  /// The type ID
  #[serde(rename = "type")]
  pub ty: TypeRef,
  /// Mutability
  #[serde(default)]
  pub mutable: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ExportFunction {
  /// Function ID
  pub id: Uuid,
  /// Function name
  pub name: String,
  /// Function parameters
  #[serde(default)]
  pub parameters: Vec<Parameter>,
  /// The return type
  pub ret: TypeRef,
}

impl ExportFunction {
  pub fn type_dependencies(&self) -> HashSet<Uuid> {
    let mut deps = HashSet::new();
    for param in &self.parameters {
      deps = deps.union(&param.ty.type_dependencies()).cloned().collect();
    }
    deps = deps.union(&self.ret.type_dependencies()).cloned().collect();
    deps
  }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ImportFunction {
  /// Module ID
  pub module: Uuid,
  /// Function ID
  pub id: Uuid,
  /// Function name
  pub name: String,
  /// Function parameters
  pub parameters: Vec<Parameter>,
  /// The return type
  pub ret: TypeRef,
}

impl ImportFunction {
  pub fn type_dependencies(&self) -> HashSet<Uuid> {
    let mut deps = HashSet::new();
    for param in &self.parameters {
      deps = deps.union(&param.ty.type_dependencies()).cloned().collect();
    }
    deps = deps.union(&self.ret.type_dependencies()).cloned().collect();
    deps
  }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ExportSymbol {
  /// A function
  Function(ExportFunction),
}

impl ExportSymbol {
  pub fn id(&self) -> &Uuid {
    match self {
      Self::Function(f) => &f.id,
    }
  }

  pub fn name(&self) -> &String {
    match self {
      Self::Function(f) => &f.name,
    }
  }

  pub fn type_dependencies(&self) -> HashSet<Uuid> {
    match self {
      Self::Function(f) => f.type_dependencies(),
    }
  }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ImportSymbol {
  /// A function
  Function(ImportFunction),
}

impl ImportSymbol {
  pub fn module(&self) -> &Uuid {
    match self {
      Self::Function(f) => &f.module,
    }
  }

  pub fn id(&self) -> &Uuid {
    match self {
      Self::Function(f) => &f.id,
    }
  }

  pub fn name(&self) -> &String {
    match self {
      Self::Function(f) => &f.name,
    }
  }

  pub fn type_dependencies(&self) -> HashSet<Uuid> {
    match self {
      Self::Function(f) => f.type_dependencies(),
    }
  }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Header {
  /// The module's ID
  pub id: Uuid,
  /// Name
  pub name: String,
  /// Author name
  pub author: String,
  /// Optional description
  pub description: Option<String>,
  /// License
  pub license: String,
  /// Semantic version of this module
  pub version: SemanticVersion,
  /// The executor (e.g., WebAssembly, Python, JavaScript, etc.)
  pub executor: Executor,
  /// Exported symbols
  pub exports: Vec<ExportSymbol>,
  /// Imported symbols
  pub imports: Vec<ImportSymbol>,
  /// MIME type of executable data (allows the same executor to support different formats
  pub executable_mime: String,
}

impl Header {
  pub fn type_dependencies(&self) -> HashSet<Uuid> {
    let mut deps = HashSet::new();
    
    for export in &self.exports {
      match export {
        ExportSymbol::Function(function) => {
          deps = deps.union(&function.ret.type_dependencies()).cloned().collect();
          for parameter in &function.parameters {
            deps = deps.union(&parameter.ty.type_dependencies()).cloned().collect();
          }
        }
      }
    }

    for import in &self.imports {
      match import {
        ImportSymbol::Function(function) => {
          deps = deps.union(&function.ret.type_dependencies()).cloned().collect();
          for parameter in &function.parameters {
            deps = deps.union(&parameter.ty.type_dependencies()).cloned().collect();
          }
        }
      }
    }

    deps
  }

  pub fn module_dependencies(&self) -> HashSet<Uuid> {
    let mut deps = HashSet::new();

    for import in &self.imports {
      deps.insert(import.module().clone());
    }

    deps
  }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ModuleDefinition {
  pub schema_version: u32,

  pub header: Header,

  /// Arbitrary data to be executed by the executor
  pub executable: Box<[u8]>,
}
