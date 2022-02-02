use std::collections::HashMap;

use serde::{Serialize, Deserialize};
use uuid::Uuid;

use crate::module::high::TypeRef;

#[derive(Debug, Serialize, Deserialize)]
pub struct StructureField {
  pub name: String,
  #[serde(rename = "type")]
  pub ty: TypeRef
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Structure {
  pub fields: HashMap<Uuid, StructureField>
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EnumerationValue {
  pub name: String,
  #[serde(rename = "type")]
  pub ty: TypeRef
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Enumeration {
  pub values: HashMap<Uuid, EnumerationValue>
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum TypeKind {
  Structure(Structure),
  Enumeration(Enumeration),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Type {
  pub name: String,
  pub description: String,
  pub kind: TypeKind
}