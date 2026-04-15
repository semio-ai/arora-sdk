use crate::ast::{Expression, ToExpression};

lazy_static::lazy_static! {
  pub static ref ARORA_BUFFER_TYPE_STRUCTURE: Expression = "ARORA_BUFFER_TYPE_STRUCTURE".to_expression();
  pub static ref ARORA_BUFFER_TYPE_ENUMERATION: Expression = "ARORA_BUFFER_TYPE_ENUMERATION".to_expression();
  pub static ref ARORA_BUFFER_TYPE_MAP: Expression = "ARORA_BUFFER_TYPE_MAP".to_expression();
  pub static ref ARORA_BUFFER_TYPE_OPTION: Expression = "ARORA_BUFFER_TYPE_OPTION".to_expression();
  pub static ref ARORA_BUFFER_TYPE_UUID: Expression = "ARORA_BUFFER_TYPE_UUID".to_expression();
  pub static ref ARORA_BUFFER_TYPE_VALUE: Expression = "ARORA_BUFFER_TYPE_VALUE".to_expression();

  pub static ref NULL_OPTION: Expression = "std::experimental::nullopt".to_expression();
}
