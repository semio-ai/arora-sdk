use std::collections::{HashMap, HashSet};

use uuid::Uuid;

use crate::module::low::TypeRef;
use crate::ty::low::{Type, TypeKind};

pub mod high;
pub mod low;

/// A set of [`low::Type`]s keyed by id, used to resolve the [`TypeRef`]s that
/// name nested types during a type-directed traversal (validation, defaulting,
/// (de)serialization). Well-known primitives are recognised by id and need no
/// entry (see [`PRIMITIVE_TYPES`]); user-defined structures and enumerations
/// must be present.
pub type TypeRegistry = HashMap<Uuid, Type>;

lazy_static::lazy_static! {
  pub static ref UNIT_ID: Uuid = Uuid::parse_str("00000000-0000-0000-0000-000000000000").unwrap();
  pub static ref BOOLEAN_ID: Uuid = Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
  pub static ref I8_ID: Uuid = Uuid::parse_str("00000000-0000-0000-0000-000000000002").unwrap();
  pub static ref I16_ID: Uuid = Uuid::parse_str("00000000-0000-0000-0000-000000000003").unwrap();
  pub static ref I32_ID: Uuid = Uuid::parse_str("00000000-0000-0000-0000-000000000004").unwrap();
  pub static ref I64_ID: Uuid = Uuid::parse_str("00000000-0000-0000-0000-000000000005").unwrap();
  pub static ref U8_ID: Uuid = Uuid::parse_str("00000000-0000-0000-0000-000000000006").unwrap();
  pub static ref U16_ID: Uuid = Uuid::parse_str("00000000-0000-0000-0000-000000000007").unwrap();
  pub static ref U32_ID: Uuid = Uuid::parse_str("00000000-0000-0000-0000-000000000008").unwrap();
  pub static ref U64_ID: Uuid = Uuid::parse_str("00000000-0000-0000-0000-000000000009").unwrap();
  pub static ref F32_ID: Uuid = Uuid::parse_str("00000000-0000-0000-0000-00000000000a").unwrap();
  pub static ref F64_ID: Uuid = Uuid::parse_str("00000000-0000-0000-0000-00000000000b").unwrap();
  pub static ref STRING_ID: Uuid = Uuid::parse_str("00000000-0000-0000-0000-00000000000c").unwrap();
  pub static ref OPTION_ID: Uuid = Uuid::parse_str("00000000-0000-0000-0000-00000000000d").unwrap();
  pub static ref ARRAY_BOOLEAN_ID: Uuid = Uuid::parse_str("00000000-0000-0000-0000-00000000000e").unwrap();
  pub static ref ARRAY_U8_ID: Uuid = Uuid::parse_str("00000000-0000-0000-0000-00000000000f").unwrap();
  pub static ref ARRAY_U16_ID: Uuid = Uuid::parse_str("00000000-0000-0000-0000-000000000010").unwrap();
  pub static ref ARRAY_U32_ID: Uuid = Uuid::parse_str("00000000-0000-0000-0000-000000000011").unwrap();
  pub static ref ARRAY_U64_ID: Uuid = Uuid::parse_str("00000000-0000-0000-0000-000000000012").unwrap();
  pub static ref ARRAY_I8_ID: Uuid = Uuid::parse_str("00000000-0000-0000-0000-000000000013").unwrap();
  pub static ref ARRAY_I16_ID: Uuid = Uuid::parse_str("00000000-0000-0000-0000-000000000014").unwrap();
  pub static ref ARRAY_I32_ID: Uuid = Uuid::parse_str("00000000-0000-0000-0000-000000000015").unwrap();
  pub static ref ARRAY_I64_ID: Uuid = Uuid::parse_str("00000000-0000-0000-0000-000000000016").unwrap();
  pub static ref ARRAY_F32_ID: Uuid = Uuid::parse_str("00000000-0000-0000-0000-000000000017").unwrap();
  pub static ref ARRAY_F64_ID: Uuid = Uuid::parse_str("00000000-0000-0000-0000-000000000018").unwrap();
  pub static ref ARRAY_STRING_ID: Uuid = Uuid::parse_str("00000000-0000-0000-0000-000000000019").unwrap();
  pub static ref ARRAY_VALUE_ID: Uuid = Uuid::parse_str("00000000-0000-0000-0000-00000000001a").unwrap();
  pub static ref KEY_VALUE_ID: Uuid = Uuid::parse_str("00000000-0000-0000-0000-00000000001b").unwrap();
  pub static ref UUID_ID: Uuid = Uuid::parse_str("00000000-0000-0000-0000-00000000001c").unwrap();

  pub static ref PRIMITIVE_IDS: HashSet<Uuid> = {
    let mut ids = HashSet::new();
    ids.insert(*UNIT_ID);
    ids.insert(*BOOLEAN_ID);
    ids.insert(*I8_ID);
    ids.insert(*I16_ID);
    ids.insert(*I32_ID);
    ids.insert(*I64_ID);
    ids.insert(*U8_ID);
    ids.insert(*U16_ID);
    ids.insert(*U32_ID);
    ids.insert(*U64_ID);
    ids.insert(*F32_ID);
    ids.insert(*F64_ID);
    ids.insert(*STRING_ID);
    ids
  };

  pub static ref WELL_KNOWN_IDS: HashSet<Uuid> = {
    let mut ids = PRIMITIVE_IDS.clone();
    ids.insert(*OPTION_ID);
    ids.insert(*ARRAY_BOOLEAN_ID);
    ids.insert(*ARRAY_U8_ID);
    ids.insert(*ARRAY_U16_ID);
    ids.insert(*ARRAY_U32_ID);
    ids.insert(*ARRAY_U64_ID);
    ids.insert(*ARRAY_I8_ID);
    ids.insert(*ARRAY_I16_ID);
    ids.insert(*ARRAY_I32_ID);
    ids.insert(*ARRAY_I64_ID);
    ids.insert(*ARRAY_F32_ID);
    ids.insert(*ARRAY_F64_ID);
    ids.insert(*ARRAY_STRING_ID);
    ids.insert(*ARRAY_VALUE_ID);
    ids.insert(*KEY_VALUE_ID);
    ids.insert(*UUID_ID);
    ids
  };

  pub static ref PRIMITIVE_LOW_TYPE_REFS: HashMap<Uuid, TypeRef> = {
    let mut types: HashMap<Uuid, TypeRef> = HashMap::new();
    let make_scalar = |id: &Uuid| TypeRef::Scalar { id: *id };
    let mut insert_scalar_id = |id: &Uuid| types.insert(*id, make_scalar(id));
    insert_scalar_id(&UNIT_ID);
    insert_scalar_id(&BOOLEAN_ID);
    insert_scalar_id(&I8_ID);
    insert_scalar_id(&I16_ID);
    insert_scalar_id(&I32_ID);
    insert_scalar_id(&I64_ID);
    insert_scalar_id(&U8_ID);
    insert_scalar_id(&U16_ID);
    insert_scalar_id(&U32_ID);
    insert_scalar_id(&U64_ID);
    insert_scalar_id(&F32_ID);
    insert_scalar_id(&F64_ID);
    insert_scalar_id(&STRING_ID); // Technically not a scalar, but a primitive type at least.
    types
  };

  pub static ref PRIMITIVE_TYPES: HashMap<Uuid, Type> = {
    let mut types: HashMap<Uuid, Type> = HashMap::new();
    let mut insert_primitive_type = |id: &Uuid, name: &str, description: &str| {
      types.insert(*id, Type {
        name: name.to_string(),
        id: *id,
        description: description.to_string(),
        kind: TypeKind::Primitive(PRIMITIVE_LOW_TYPE_REFS.get(id).unwrap().clone()),
      });

    };
    insert_primitive_type(&UNIT_ID, "unit", "a.k.a. \"nothing\"");
    insert_primitive_type(&BOOLEAN_ID, "boolean", "either true or false");
    insert_primitive_type(&I8_ID, "i8", "8-bit signed integer");
    insert_primitive_type(&I16_ID, "i16", "16-bit signed integer");
    insert_primitive_type(&I32_ID, "i32", "32-bit signed integer");
    insert_primitive_type(&I64_ID, "i64", "64-bit signed integer");
    insert_primitive_type(&U8_ID, "u8", "8-bit unsigned integer");
    insert_primitive_type(&U16_ID, "u16", "16-bit unsigned integer");
    insert_primitive_type(&U32_ID, "u32", "32-bit unsigned integer");
    insert_primitive_type(&U64_ID, "u64", "64-bit unsigned integer");
    insert_primitive_type(&F32_ID, "f32", "32-bit floating point decimal, a.k.a. \"float\"");
    insert_primitive_type(&F64_ID, "f64", "64-bit floating point decimal, a.k.a. \"double\"");
    insert_primitive_type(&STRING_ID, "str", "a string of u8 characters");
    types
  };
}
