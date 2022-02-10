use std::collections::{HashSet, HashMap};

use uuid::Uuid;

use crate::module::low::TypeRef;
use crate::ty::low::{Type, TypeKind};

pub mod high;
pub mod low;

lazy_static::lazy_static! {
  pub static ref UNIT_ID: Uuid = Uuid::parse_str("00000000-0000-0000-0000-000000000000").unwrap();
  pub static ref BOOLEAN_ID: Uuid = Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
  pub static ref S8_ID: Uuid = Uuid::parse_str("00000000-0000-0000-0000-000000000002").unwrap();
  pub static ref S16_ID: Uuid = Uuid::parse_str("00000000-0000-0000-0000-000000000003").unwrap();
  pub static ref S32_ID: Uuid = Uuid::parse_str("00000000-0000-0000-0000-000000000004").unwrap();
  pub static ref S64_ID: Uuid = Uuid::parse_str("00000000-0000-0000-0000-000000000005").unwrap();
  pub static ref U8_ID: Uuid = Uuid::parse_str("00000000-0000-0000-0000-000000000006").unwrap();
  pub static ref U16_ID: Uuid = Uuid::parse_str("00000000-0000-0000-0000-000000000007").unwrap();
  pub static ref U32_ID: Uuid = Uuid::parse_str("00000000-0000-0000-0000-000000000008").unwrap();
  pub static ref U64_ID: Uuid = Uuid::parse_str("00000000-0000-0000-0000-000000000009").unwrap();
  pub static ref R32_ID: Uuid = Uuid::parse_str("00000000-0000-0000-0000-00000000000a").unwrap();
  pub static ref R64_ID: Uuid = Uuid::parse_str("00000000-0000-0000-0000-00000000000b").unwrap();
  pub static ref STRING_ID: Uuid = Uuid::parse_str("00000000-0000-0000-0000-00000000000c").unwrap();
  pub static ref ARRAY_ID: Uuid = Uuid::parse_str("00000000-0000-0000-0000-00000000000f").unwrap();
  pub static ref MAP_ID: Uuid = Uuid::parse_str("00000000-0000-0000-0000-000000000010").unwrap();

  pub static ref PRIMITIVE_IDS: HashSet<Uuid> = {
    let mut ids = HashSet::new();
    ids.insert(UNIT_ID.clone());
    ids.insert(BOOLEAN_ID.clone());
    ids.insert(S8_ID.clone());
    ids.insert(S16_ID.clone());
    ids.insert(S32_ID.clone());
    ids.insert(S64_ID.clone());
    ids.insert(U8_ID.clone());
    ids.insert(U16_ID.clone());
    ids.insert(U32_ID.clone());
    ids.insert(U64_ID.clone());
    ids.insert(R32_ID.clone());
    ids.insert(R64_ID.clone());
    ids.insert(STRING_ID.clone());
    ids.insert(ARRAY_ID.clone());
    ids.insert(MAP_ID.clone());
    ids
  };
  
  pub static ref PRIMITIVE_LOW_TYPE_REFS: HashMap<Uuid, TypeRef> = {
    let mut types: HashMap<Uuid, TypeRef> = HashMap::new();
    let make_scalar = |id: &Uuid| TypeRef::Scalar { id: id.clone() };
    let mut insert_scalar_id = |id: &Uuid| types.insert(id.clone(), make_scalar(id));
    insert_scalar_id(&UNIT_ID);
    insert_scalar_id(&BOOLEAN_ID);
    insert_scalar_id(&S8_ID);
    insert_scalar_id(&S16_ID);
    insert_scalar_id(&S32_ID);
    insert_scalar_id(&S64_ID);
    insert_scalar_id(&U8_ID);
    insert_scalar_id(&U16_ID);
    insert_scalar_id(&U32_ID);
    insert_scalar_id(&U64_ID);
    insert_scalar_id(&R32_ID);
    insert_scalar_id(&R64_ID);
    insert_scalar_id(&STRING_ID); // Technically not a scalar, but a primitive type at least.
    types
  };

  pub static ref PRIMITIVE_TYPES: HashMap<Uuid, Type> = {
    let mut types: HashMap<Uuid, Type> = HashMap::new();
    let mut insert_primitive_type = |id: &Uuid, name: &str, description: &str| {
      types.insert(id.clone(), Type {
        name: name.to_string(),
        id: id.clone(),
        description: description.to_string(),
        kind: TypeKind::Primitive(PRIMITIVE_LOW_TYPE_REFS.get(id).unwrap().clone()),
      });
      ()
    };
    insert_primitive_type(&UNIT_ID, "unit", "a.k.a. \"nothing\"");
    insert_primitive_type(&BOOLEAN_ID, "boolean", "either true or false");
    insert_primitive_type(&S8_ID, "s8", "8-bit signed integer");
    insert_primitive_type(&S16_ID, "s16", "16-bit signed integer");
    insert_primitive_type(&S32_ID, "s32", "32-bit signed integer");
    insert_primitive_type(&S64_ID, "s64", "64-bit signed integer");
    insert_primitive_type(&U8_ID, "u8", "8-bit unsigned integer");
    insert_primitive_type(&U16_ID, "u16", "16-bit unsigned integer");
    insert_primitive_type(&U32_ID, "u32", "32-bit unsigned integer");
    insert_primitive_type(&U64_ID, "u64", "64-bit unsigned integer");
    insert_primitive_type(&R32_ID, "r32", "32-bit floating point decimal, a.k.a. \"float\"");
    insert_primitive_type(&R64_ID, "r64", "64-bit floating point decimal, a.k.a. \"double\"");
    insert_primitive_type(&STRING_ID, "str", "a string of u8 characters");
    types
  };
}
