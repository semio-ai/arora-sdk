use crate::{SemanticVersion, value::Value};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug)]
pub struct Executor {
  /// The executor name (e.g., WebAssembly, Python, Javascript, etc.)
  pub name: String,
  pub min_version: Option<SemanticVersion>,
  pub max_version: Option<SemanticVersion>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum TypeRef {
  Scalar { id: String },
  Array { id: String },
  Map { key_id: String, value_id: String },
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Parameter {
  /// ID
  pub id: Uuid,
  /// Name
  pub name: String,
  /// The type
  #[serde(rename = "type")]
  pub ty: TypeRef,
  /// Mutability
  #[serde(default)]
  pub mutable: bool,
  /// Default value
  pub default_value: Option<Value>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ImportFunction {
  /// Module ID
  pub module: String,
  /// Function ID
  pub id: Uuid,
  /// Function name
  pub name: String,
  /// Function parameters
  pub parameters: Vec<Parameter>,
  /// The return type
  pub ret: TypeRef,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ExportFunction {
  /// Function ID
  pub id: Uuid,
  /// Function name
  pub name: String,
  /// Function parameters
  #[serde(default)]
  pub parameters: Vec<Parameter>,
  /// The return type
  #[serde(default = "default_return_type")]
  pub ret: TypeRef,
}

fn default_return_type() -> TypeRef {
  TypeRef::Scalar { id: "unit".to_string() }
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ImportSymbol {
  /// A function
  Function(ImportFunction),
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ExportSymbol {
  /// A function
  Function(ExportFunction),
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
  pub exports: Vec<ExportSymbol>,
  /// Imported symbols
  pub imports: Vec<ImportSymbol>,
  /// MIME type of executable data (allows the same executor to support different formats
  pub executable_mime: String,
}
