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
  /// The type
  #[serde(rename = "type")]
  pub ty: String,
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
  pub ret: String,
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
  /// Module name
  pub name: String,
  /// Minimum version
  pub min_version: Option<SemanticVersion>,
  /// Maximum version
  pub max_version: Option<SemanticVersion>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ModuleDefinition {
  /// The module's ID
  pub id: Uuid,

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
