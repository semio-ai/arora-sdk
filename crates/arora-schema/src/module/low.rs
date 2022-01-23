use std::collections::HashSet;

use crate::SemanticVersion;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug)]
pub struct Executor {
  /// The executor name (e.g., WebAssembly, Python, Javascript, etc.)
  pub name: String,
  pub min_version: Option<SemanticVersion>,
  pub max_version: Option<SemanticVersion>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Parameter {
  /// ID
  pub id: Uuid,
  /// Name
  pub name: String,
  /// The type ID
  #[serde(rename = "type_id")]
  pub ty_id: Uuid,
  /// Mutability
  #[serde(default)]
  pub mutable: bool,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Function {
  /// Function ID
  pub id: Uuid,
  /// Function name
  pub name: String,
  /// Function parameters
  pub parameters: Vec<Parameter>,
  /// The return type
  pub ret: Uuid,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Node {
  /// Node ID
  pub id: Uuid,
  /// The node's name
  pub name: String,
  /// Parameters
  pub parameters: Vec<Parameter>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Symbol {
  /// A function
  Function(Function),
  /// A behavior tree node
  Node(Node),
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Dependency {
  /// Module id
  pub id: Uuid,
  /// Minimum version
  pub min_version: Option<SemanticVersion>,
  /// Maximum version
  pub max_version: Option<SemanticVersion>,
}

#[derive(Serialize, Deserialize, Debug)]
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
  pub exports: Vec<Symbol>,
  /// Imported symbols
  pub imports: Vec<Symbol>,
  /// Required dependencies
  pub dependencies: Vec<Dependency>,
  /// MIME type of executable data (allows the same executor to support different formats
  pub executable_mime: String,
}

impl Header {
  pub fn type_dependencies(&self) -> HashSet<Uuid> {
    let mut deps = HashSet::new();
    for export in &self.exports {
      match export {
        Symbol::Function(function) => {
          deps.insert(function.ret);
          for parameter in &function.parameters {
            deps.insert(parameter.ty_id);
          }
        }
        Symbol::Node(node) => {
          for parameter in &node.parameters {
            deps.insert(parameter.ty_id);
          }
        }
      }
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
