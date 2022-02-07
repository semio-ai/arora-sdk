use arora_schema::ty::{
  BOOLEAN_ID, R32_ID, R64_ID, S16_ID, S32_ID, S64_ID, S8_ID, STRING_ID, U16_ID, U32_ID, U64_ID,
  U8_ID, UNIT_ID,
};

use crate::{ast::TypeRef, Context};

lazy_static::lazy_static! {
  pub static ref VOID: TypeRef = TypeRef {
    ty: "void".to_string(),
    ..Default::default()
  };

  pub static ref BOOL: TypeRef = TypeRef {
    ty: "bool".to_string(),
    ..Default::default()
  };

  pub static ref U8: TypeRef = TypeRef {
    ty: "uint8_t".to_string(),
    ..Default::default()
  };

  pub static ref U8_PTR: TypeRef = TypeRef {
    ty: "uint8_t".to_string(),
    pointer: true,
    ..Default::default()
  };

  pub static ref U8_CONST: TypeRef = TypeRef {
    ty: "uint8_t".to_string(),
    constant: true,
    ..Default::default()
  };

  pub static ref U8_CONST_PTR: TypeRef = TypeRef {
    ty: "uint8_t".to_string(),
    pointer: true,
    constant: true,
    ..Default::default()
  };

  pub static ref U32: TypeRef = TypeRef {
    ty: "uint32_t".to_string(),
    ..Default::default()
  };

  pub static ref ARORA_BUFFER_READER_PTR: TypeRef = TypeRef {
    ty: "arora_buffer_reader".to_string(),
    pointer: true,
    ..Default::default()
  };

  pub static ref ARORA_BUFFER_WRITER_PTR: TypeRef = TypeRef {
    ty: "arora_buffer_writer".to_string(),
    pointer: true,
    ..Default::default()
  };

  pub static ref ARORA_GET_STRUCTURE_RESULT: TypeRef = TypeRef {
    ty: "arora_get_structure_result".to_string(),
    ..Default::default()
  };

  pub static ref ARORA_GET_ENUMERATION_VALUE_RESULT: TypeRef = TypeRef {
    ty: "arora_get_enumeration_value_result".to_string(),
    ..Default::default()
  };
}

pub fn type_name<'a>(context: &'a Context<'a>, ty: &arora_schema::module::low::TypeRef) -> String {
  match ty {
    arora_schema::module::low::TypeRef::Scalar { id } => match id {
      x if *x == *UNIT_ID => "void".to_string(),
      x if *x == *BOOLEAN_ID => "bool".to_string(),
      x if *x == *U8_ID => "std::uint8_t".to_string(),
      x if *x == *U16_ID => "std::uint16_t".to_string(),
      x if *x == *U32_ID => "std::uint32_t".to_string(),
      x if *x == *U64_ID => "std::uint64_t".to_string(),
      x if *x == *S8_ID => "std::int8_t".to_string(),
      x if *x == *S16_ID => "std::int16_t".to_string(),
      x if *x == *S32_ID => "std::int32_t".to_string(),
      x if *x == *S64_ID => "std::int64_t".to_string(),
      x if *x == *R32_ID => "float".to_string(),
      x if *x == *R64_ID => "double".to_string(),
      x if *x == *STRING_ID => "std::string_view".to_string(),
      x => {
        let ty = context.types.get(&x).expect(format!("encountered unknown type {}", x).as_str());
        ty.name.clone()
      }
    },
    arora_schema::module::low::TypeRef::Array { id } => match id {
      x if *x == *BOOLEAN_ID => "arora::buffer::View<bool>".to_string(),
      x if *x == *U8_ID => "arora::buffer::View<std::uint8_t>".to_string(),
      x if *x == *U16_ID => "arora::buffer::View<std::uint16_t>".to_string(),
      x if *x == *U32_ID => "arora::buffer::View<std::uint32_t>".to_string(),
      x if *x == *U64_ID => "arora::buffer::View<std::uint64_t>".to_string(),
      x if *x == *S8_ID => "arora::buffer::View<std::int8_t>".to_string(),
      x if *x == *S16_ID => "arora::buffer::View<std::int16_t>".to_string(),
      x if *x == *S32_ID => "arora::buffer::View<std::int32_t>".to_string(),
      x if *x == *S64_ID => "arora::buffer::View<std::int64_t>".to_string(),
      x if *x == *R32_ID => "arora::buffer::View<float>".to_string(),
      x if *x == *R64_ID => "arora::buffer::View<double>".to_string(),
      x if *x == *STRING_ID => "std::vector<std::string_view>".to_string(),
      x => {
        let ty = context.types.get(&x).unwrap();
        format!("std::vector<{}>", ty.name)
      }
    },
    arora_schema::module::low::TypeRef::Map { key_id, value_id } => {
      let key_ty = context.types.get(&key_id).unwrap();
      let value_ty = context.types.get(&value_id).unwrap();
      format!("std::unordered_map<{}, {}>", key_ty.name, value_ty.name)
    }
  }
}

pub fn optional(ty: &TypeRef) -> TypeRef {
  TypeRef {
    ty: "std::optional".to_string(),
    arguments: Some(vec![ty.clone()]),
    ..Default::default()
  }
}

pub fn optional_const(ty: &TypeRef) -> TypeRef {
  TypeRef {
    ty: "std::optional".to_string(),
    arguments: Some(vec![ty.clone()]),
    constant: true,
    ..Default::default()
  }
}

pub fn optional_ptr(ty: &TypeRef) -> TypeRef {
  TypeRef {
    ty: "std::optional".to_string(),
    arguments: Some(vec![ty.clone()]),
    constant: true,
    pointer: true,
    ..Default::default()
  }
}

pub fn optional_const_ptr(ty: &TypeRef) -> TypeRef {
  TypeRef {
    ty: "std::optional".to_string(),
    arguments: Some(vec![ty.clone()]),
    constant: true,
    pointer: true,
    ..Default::default()
  }
}

pub fn optional_const_ref(ty: &TypeRef) -> TypeRef {
  TypeRef {
    ty: "std::optional".to_string(),
    arguments: Some(vec![ty.clone()]),
    constant: true,
    reference: true,
    ..Default::default()
  }
}

pub fn optional_ref(ty: &TypeRef) -> TypeRef {
  TypeRef {
    ty: "std::optional".to_string(),
    arguments: Some(vec![ty.clone()]),
    reference: true,
    ..Default::default()
  }
}

pub fn optional_move(ty: &TypeRef) -> TypeRef {
  TypeRef {
    ty: "std::optional".to_string(),
    arguments: Some(vec![ty.clone()]),
    rvalue_reference: true,
    ..Default::default()
  }
}
