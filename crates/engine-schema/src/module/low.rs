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
  pub name: String,
  /// The type
  pub ty: String,
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
  pub ret: String,
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
pub struct Header {
  /// The module's ID
  pub id: Uuid,
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
  /// Artifact path
  pub artifact_path: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ModuleDefinition {
  pub schema_version: u32,

  pub header: Header,

  /// Arbitrary data to be executed by the executor
  pub executable: Box<[u8]>,
}
