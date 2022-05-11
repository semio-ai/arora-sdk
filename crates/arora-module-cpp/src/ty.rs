use crate::{ast::TypeRef, Context};
use semio_record::ty::{FrozenTy, PrimitiveKind};

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

pub fn type_name<'a>(context: &'a Context<'a>, ty: &FrozenTy) -> String {
  match ty {
    FrozenTy::Primitive(primitive) => match primitive.kind {
      PrimitiveKind::Unit => "void".to_string(),
      PrimitiveKind::Boolean => "bool".to_string(),
      PrimitiveKind::U8 => "std::uint8_t".to_string(),
      PrimitiveKind::U16 => "std::uint16_t".to_string(),
      PrimitiveKind::U32 => "std::uint32_t".to_string(),
      PrimitiveKind::U64 => "std::uint64_t".to_string(),
      PrimitiveKind::I8 => "std::int8_t".to_string(),
      PrimitiveKind::I16 => "std::int16_t".to_string(),
      PrimitiveKind::I32 => "std::int32_t".to_string(),
      PrimitiveKind::I64 => "std::int64_t".to_string(),
      PrimitiveKind::F32 => "float".to_string(),
      PrimitiveKind::F64 => "double".to_string(),
      PrimitiveKind::String => "std::string".to_string(),
      PrimitiveKind::ArrayBoolean => "std::vector<bool>".to_string(),
      PrimitiveKind::ArrayU8 => "std::vector<std::uint8_t>".to_string(),
      PrimitiveKind::ArrayU16 => "std::vector<std::uint16_t>".to_string(),
      PrimitiveKind::ArrayU32 => "std::vector<std::uint32_t>".to_string(),
      PrimitiveKind::ArrayU64 => "std::vector<std::uint64_t>".to_string(),
      PrimitiveKind::ArrayI8 => "std::vector<std::int8_t>".to_string(),
      PrimitiveKind::ArrayI16 => "std::vector<std::int16_t>".to_string(),
      PrimitiveKind::ArrayI32 => "std::vector<std::int32_t>".to_string(),
      PrimitiveKind::ArrayI64 => "std::vector<std::int64_t>".to_string(),
      PrimitiveKind::ArrayF32 => "std::vector<float>".to_string(),
      PrimitiveKind::ArrayF64 => "std::vector<double>".to_string(),
      PrimitiveKind::ArrayString => "std::vector<std::string>".to_string(),
    },
    FrozenTy::FrozenScalar(scalar) => {
      let ty = context
        .types
        .get(&scalar.reference.id)
        .expect(format!("encountered unknown type {}", scalar.reference.id).as_str());
      ty.name().clone()
    }
    FrozenTy::FrozenArray(array) => {
      let ty = context.types.get(&array.reference.id).unwrap();
      format!("std::vector<{}>", ty.name())
    }
  }
}

pub const OPTIONAL_TYPENAME: &str = "std::experimental::optional";

pub fn optional(ty: &TypeRef) -> TypeRef {
  TypeRef {
    ty: OPTIONAL_TYPENAME.to_string(),
    arguments: Some(vec![ty.clone()]),
    ..Default::default()
  }
}

pub fn optional_const(ty: &TypeRef) -> TypeRef {
  TypeRef {
    ty: OPTIONAL_TYPENAME.to_string(),
    arguments: Some(vec![ty.clone()]),
    constant: true,
    ..Default::default()
  }
}

pub fn optional_ptr(ty: &TypeRef) -> TypeRef {
  TypeRef {
    ty: OPTIONAL_TYPENAME.to_string(),
    arguments: Some(vec![ty.clone()]),
    constant: true,
    pointer: true,
    ..Default::default()
  }
}

pub fn optional_const_ptr(ty: &TypeRef) -> TypeRef {
  TypeRef {
    ty: OPTIONAL_TYPENAME.to_string(),
    arguments: Some(vec![ty.clone()]),
    constant: true,
    pointer: true,
    ..Default::default()
  }
}

pub fn optional_const_ref(ty: &TypeRef) -> TypeRef {
  TypeRef {
    ty: OPTIONAL_TYPENAME.to_string(),
    arguments: Some(vec![ty.clone()]),
    constant: true,
    reference: true,
    ..Default::default()
  }
}

pub fn optional_ref(ty: &TypeRef) -> TypeRef {
  TypeRef {
    ty: OPTIONAL_TYPENAME.to_string(),
    arguments: Some(vec![ty.clone()]),
    reference: true,
    ..Default::default()
  }
}

pub fn optional_move(ty: &TypeRef) -> TypeRef {
  TypeRef {
    ty: OPTIONAL_TYPENAME.to_string(),
    arguments: Some(vec![ty.clone()]),
    rvalue_reference: true,
    ..Default::default()
  }
}
