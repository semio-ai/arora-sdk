use arora_schema::value::{ConversionError, Enumeration, Value};
use uuid::Uuid;

#[derive(Debug, PartialEq)]
pub enum Status {
  Success,
  Failure,
  Running,
}

pub const STATUS_TYPE_ID: Uuid = Uuid::from_bytes([
  0x32, 0x5a, 0x57, 0x67, 0xe3, 0x44, 0x45, 0x32, 0x86, 0x0e, 0x07, 0x49, 0xbc, 0xf2, 0xe4, 0x28,
]);
pub const STATUS_SUCCESS_VARIANT_ID: Uuid = Uuid::from_bytes([
  0x76, 0x6e, 0x9e, 0x9a, 0x44, 0x6d, 0x4e, 0x46, 0x83, 0xe6, 0x14, 0xb7, 0xca, 0x10, 0x11, 0x69,
]);
pub const STATUS_FAILURE_VARIANT_ID: Uuid = Uuid::from_bytes([
  0x24, 0x68, 0xf4, 0x6c, 0xbb, 0x60, 0x42, 0x5c, 0x9a, 0x4d, 0x9a, 0xd3, 0x26, 0xcc, 0xc7, 0xe2,
]);
pub const STATUS_RUNNING_VARIANT_ID: Uuid = Uuid::from_bytes([
  0xac, 0xd7, 0x9e, 0xc6, 0x0c, 0x44, 0x40, 0x1a, 0x82, 0xf8, 0x5d, 0xa5, 0x42, 0x2d, 0x3e, 0xec,
]);

impl TryFrom<Value> for Status {
  type Error = ConversionError;

  fn try_from(value: Value) -> Result<Self, Self::Error> {
    if let Value::Enumeration(as_enum) = value {
      if as_enum.id == STATUS_TYPE_ID {
        match as_enum.variant_id {
          STATUS_SUCCESS_VARIANT_ID => Ok(Status::Success),
          STATUS_FAILURE_VARIANT_ID => Ok(Status::Failure),
          STATUS_RUNNING_VARIANT_ID => Ok(Status::Running),
          _ => Err(Self::Error {
            message: "unexpected variant ID".to_string(),
          }),
        }
      } else {
        Err(Self::Error {
          message: "unexpected enum type ID".to_string(),
        })
      }
    } else {
      Err(Self::Error {
        message: "unexpected kind".to_string(),
      })
    }
  }
}

impl Into<Value> for Status {
  fn into(self) -> Value {
    let variant_id = match self {
      Status::Success => STATUS_SUCCESS_VARIANT_ID,
      Status::Failure => STATUS_FAILURE_VARIANT_ID,
      Status::Running => STATUS_RUNNING_VARIANT_ID,
    };
    Value::Enumeration(Enumeration {
      id: STATUS_TYPE_ID,
      variant_id,
      value: Box::new(Value::Unit),
    })
  }
}
