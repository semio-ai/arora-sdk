use std::collections::HashSet;

use uuid::Uuid;

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
}

