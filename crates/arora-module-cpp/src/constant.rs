use crate::ast::{Expression, ToExpression};

lazy_static::lazy_static! {
  pub static ref ARORA_BUFFER_TYPE_STRUCTURE: Expression = "ARORA_BUFFER_TYPE_STRUCTURE".to_expression();
  pub static ref ARORA_BUFFER_TYPE_ENUMERATION: Expression = "ARORA_BUFFER_TYPE_ENUMERATION".to_expression();

  pub static ref NULL_OPTION: Expression = "std::nullopt".to_expression();
}
