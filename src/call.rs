use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::value::{StructureField, Value};

/// A call is described like a structure in arora engine.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Call {
  /// The ID of the module where to find the function ID.
  /// If absent, look for it locally.
  #[serde(default)]
  pub module_id: Option<Uuid>,
  /// The function ID to call.
  pub id: Uuid,
  /// Arguments to call the functions with.
  #[serde(default)]
  pub args: Vec<StructureField>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CallResult {
  pub ret: Value,
  #[serde(default)]
  pub mutated: Vec<StructureField>,
}

#[cfg(test)]
mod tests {
  use crate::value::{Structure, Value};

  use super::*;
  use std::str::FromStr;
  use uuid::Uuid;

  #[test]
  pub fn parse_call_test() {
    let call: Call = serde_yaml::from_str(CALL_TEST).unwrap();
    assert_eq!(
      call.id,
      Uuid::from_str("07f5740c-ba4a-45af-8ec5-bedde5737e99").unwrap()
    );
    if let Value::Structure(Structure { id, fields }) = &call.args[1].value.as_ref() {
      assert_eq!(
        *id,
        Uuid::from_str("7f9aedf8-dbde-4020-b5f4-c28a6635ae7c").unwrap()
      );
      if let Value::I32(v) = fields[1].value.as_ref() {
        assert_eq!(*v, 113);
      } else {
        panic!("expected i32 value under second field of struct arg");
      }
    } else {
      panic!("expected a string under arg 55dbec70-1c3a-433e-a6e6-27446b7f065e");
    }
  }

  #[test]
  pub fn parse_call_test_2() {
    let call: Call = serde_yaml::from_str(CALL_TEST_2).unwrap();
    assert_eq!(
      call.id,
      Uuid::from_str("b213a552-77ad-465a-a26d-352e8eccfd63").unwrap()
    );
    assert_eq!(call.args.len(), 2);
  }

  pub const CALL_TEST: &str = "\
id: 07f5740c-ba4a-45af-8ec5-bedde5737e99
args:
- id: b41899c3-66dc-40d4-ab61-d1ccf5231c88
  value:
    enum:
      id: 325a5767-e344-4532-860e-0749bcf2e428
      variant_id: 766e9e9a-446d-4e46-83e6-14b7ca101169
      value: unit
- id: 63086e48-804f-403a-8862-3358ddedc08d
  value:
    struct:
      id: 7f9aedf8-dbde-4020-b5f4-c28a6635ae7c
      fields:
      - id: 7d94a956-e50d-4cc4-9714-f62e1f9b134e
        value:
          enums:
            id: 325a5767-e344-4532-860e-0749bcf2e428
            elements:
              - variant_id: 2468f46c-bb60-425c-9a4d-9ad326ccc7e2
                value: unit
      - id: 5ffa9104-1e5c-4026-943f-8db38bd34563
        value:
          i32: 113
";

  pub const CALL_TEST_2: &str = "\
id: b213a552-77ad-465a-a26d-352e8eccfd63
args:
- id: 55dbec70-1c3a-433e-a6e6-27446b7f065e
  value:
    u32: 42
- id: abf9ca4e-e03f-431a-a32b-4911f809c399
  value:
    u32: 64
";
}
