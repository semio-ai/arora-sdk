use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::module::low::TypeRef;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct StructureField {
  pub name: String,
  #[serde(rename = "type")]
  pub type_ref: TypeRef,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Structure {
  pub fields: HashMap<Uuid, StructureField>,
}

impl Structure {
  pub fn type_dependencies(&self) -> HashSet<Uuid> {
    let mut deps = HashSet::new();
    for (_, value) in &self.fields {
      deps.extend(value.type_ref.type_dependencies());
    }
    deps
  }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct EnumerationValue {
  pub name: String,
  #[serde(rename = "type")]
  pub type_ref: TypeRef,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Enumeration {
  pub values: HashMap<Uuid, EnumerationValue>,
}

impl Enumeration {
  pub fn type_dependencies(&self) -> HashSet<Uuid> {
    let mut deps = HashSet::new();
    for (_, value) in &self.values {
      deps.extend(value.type_ref.type_dependencies());
    }
    deps
  }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum TypeKind {
  Structure(Structure),
  Enumeration(Enumeration),
}

impl TypeKind {
  pub fn type_dependencies(&self) -> HashSet<Uuid> {
    match self {
      Self::Structure(s) => s.type_dependencies(),
      Self::Enumeration(e) => e.type_dependencies(),
    }
  }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Type {
  pub name: String,
  pub id: Uuid,
  pub description: String,
  pub kind: TypeKind,
}

impl Type {
  pub fn type_dependencies(&self) -> HashSet<Uuid> {
    self.kind.type_dependencies()
  }
}
