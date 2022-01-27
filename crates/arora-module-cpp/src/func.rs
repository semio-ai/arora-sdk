use crate::ast::{TypeRef, Expression, ToExpression};

lazy_static::lazy_static! {
  pub static ref ASSERT: Expression = "assert".to_expression();
  pub static ref ARORA_BUFFER_READER_NEW: Expression = "arora_buffer_reader_new".to_expression();
  pub static ref ARORA_BUFFER_READER_FREE: Expression = "arora_buffer_reader_free".to_expression();
  pub static ref ARORA_BUFFER_READER_GET_STRUCTURE: Expression = "arora_buffer_reader_get_structure".to_expression();
  pub static ref ARORA_BUFFER_READER_GET_STRUCTURE_FIELD: Expression = "arora_buffer_reader_get_structure_field".to_expression();
  pub static ref ARORA_BUFFER_READER_NEXT_TYPE: Expression = "arora_buffer_reader_next_type".to_expression();
  
  pub static ref ARORA_BUFFER_WRITER_NEW: Expression = "arora_buffer_writer_new".to_expression();
  pub static ref ARORA_BUFFER_WRITER_FREE: Expression = "arora_buffer_writer_free".to_expression();
  pub static ref ARORA_BUFFER_WRITER_ADD_STRUCTURE_FIELD: Expression = "arora_buffer_writer_add_structure_field".to_expression();
  pub static ref ARORA_BUFFER_WRITER_BEGIN_STRUCTURE: Expression = "arora_buffer_writer_begin_structure".to_expression();
  pub static ref ARORA_UUID_COMPARE: Expression = "arora_uuid_compare".to_expression();
}