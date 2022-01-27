use uuid::Uuid;

use crate::{ast::{Variable, ArrayKind, Expression, Statement, ToPrettyString, ToExpression}, id, ty, func};

fn uuid_initializer_list(uuid: &Uuid) -> Expression {
  let id_bytes = uuid.as_bytes();
  Expression::InitializerList(
    vec![
      Expression::IntegerLiteral(id_bytes[0] as i64),
      Expression::IntegerLiteral(id_bytes[1] as i64),
      Expression::IntegerLiteral(id_bytes[2] as i64),
      Expression::IntegerLiteral(id_bytes[3] as i64),
      Expression::IntegerLiteral(id_bytes[4] as i64),
      Expression::IntegerLiteral(id_bytes[5] as i64),
      Expression::IntegerLiteral(id_bytes[6] as i64),
      Expression::IntegerLiteral(id_bytes[7] as i64),
      Expression::IntegerLiteral(id_bytes[8] as i64),
      Expression::IntegerLiteral(id_bytes[9] as i64),
      Expression::IntegerLiteral(id_bytes[10] as i64),
      Expression::IntegerLiteral(id_bytes[11] as i64),
      Expression::IntegerLiteral(id_bytes[12] as i64),
      Expression::IntegerLiteral(id_bytes[13] as i64),
      Expression::IntegerLiteral(id_bytes[14] as i64),
      Expression::IntegerLiteral(id_bytes[15] as i64),
    ]
  )
}

pub fn uuid_variable(name: String, uuid: &Uuid) -> Variable {
  Variable {
    name,
    ty: ty::U8_CONST.clone(),
    value: uuid_initializer_list(uuid).into(),
    array: ArrayKind::Fixed(16),
    ..Default::default()
  }
}

pub fn arora_buffer_writer() -> Variable {
  Variable {
    name: "writer".to_string(),
    ty: ty::ARORA_BUFFER_WRITER_PTR.clone(),
    value: Some(func::ARORA_BUFFER_WRITER_NEW.call::<String, _>([])),
    ..Default::default()
  }
}

pub fn arora_buffer_reader(identifier: &str) -> Variable {
  Variable {
    name: "reader".to_string(),
    ty: ty::ARORA_BUFFER_READER_PTR.clone(),
    value: Some(func::ARORA_BUFFER_READER_NEW.call([ identifier ])),
    ..Default::default()
  }
}

pub fn arora_buffer_writer_free() -> Statement {
  func::ARORA_BUFFER_WRITER_FREE.call([ "writer" ]).into_statement()
}

pub fn arora_buffer_reader_free() -> Statement {
  func::ARORA_BUFFER_READER_FREE.call([ "reader" ]).into_statement()
}

pub fn arora_buffer_writer_begin_structure(uuid_identifier: &str, field_count: u32) -> Statement {
  func::ARORA_BUFFER_WRITER_BEGIN_STRUCTURE.call([ "writer", uuid_identifier, &field_count.to_string() ]).into_statement()
}

pub fn arora_buffer_writer_add_structure_field(field_identifier: &str) -> Statement {
  func::ARORA_BUFFER_WRITER_ADD_STRUCTURE_FIELD.call([ "writer", &field_identifier ]).into_statement()
}

pub fn serialize(type_name: &str, value: &Expression) -> Statement {
  format!("arora::buffer::serialize<{}>()(writer, {})", type_name, value.to_pretty_string(0)).to_expression().into_statement()
}

pub fn deserialize(identifier: &str, type_name: &str) -> Expression {
  format!("arora::buffer::deserialize<{}>()(reader)", type_name).to_expression()
}

// ARORA_BUFFER_READER_GET_STRUCTURE_FIELD
pub fn arora_buffer_reader_get_structure_field() -> Expression {
  func::ARORA_BUFFER_READER_GET_STRUCTURE_FIELD.call([ "reader" ])
}

// arora_buffer_reader_next_type
pub fn arora_buffer_reader_next_type() -> Expression {
  func::ARORA_BUFFER_READER_NEXT_TYPE.call([ "reader" ])
}

pub fn assert(expression: Expression) -> Statement {
  func::ASSERT.call([ expression ]).into_statement()
}

// ARORA_BUFFER_READER_GET_STRUCTURE
pub fn arora_buffer_reader_get_structure() -> Expression {
  func::ARORA_BUFFER_READER_GET_STRUCTURE.call([ "reader" ])
}