use crate::SemanticVersion;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct Executor {
  /// The executor name (e.g., WebAssembly, Python, Javascript, etc.)
  pub name: String,
  pub min_version: Option<SemanticVersion>,
  pub max_version: Option<SemanticVersion>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct TypeRef {
  /// Name of type
  pub name: String,
  /// ABI version
  pub version: u32,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Parameter {
  pub name: String,
  /// The type
  pub ty: TypeRef,
  /// Mutability
  pub mutable: bool,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Function {
  /// Function name
  pub name: String,
  /// Function parameters
  pub parameters: Vec<Parameter>,
  /// The return type
  pub ret: TypeRef,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Node {
  /// The node's name
  pub name: String,
  /// Parameters
  pub parameters: Vec<Parameter>,
}

#[derive(Serialize, Deserialize, Debug)]
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
  pub id: u128,

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
