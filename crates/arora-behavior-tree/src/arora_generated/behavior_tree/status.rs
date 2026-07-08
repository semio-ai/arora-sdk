use crate::arora_generated::error::DeserializationError;
use arora_buffers::*;
use arora_types::value::{ConversionError, Enumeration, Value};
use uuid::Uuid;
#[derive(Debug, PartialEq, Clone)]
pub enum Status {
    Success,
    Failure,
    Running,
}
impl Into<Box<[u8]>> for Status {
    fn into(self) -> Box<[u8]> {
        let mut writer = BufferWriter::new();
        serialize_to_writer(&self, &mut writer);
        writer.finalize()
    }
}
pub fn serialize_to_writer(value: &Status, writer: &mut BufferWriter) {
    let enumeration_id = STATUS_ENUM_RAW_ID.as_slice();
    let variant_id = match value {
        Status::Success => STATUS_SUCCESS_VARIANT_RAW_ID.as_slice(),
        Status::Failure => STATUS_FAILURE_VARIANT_RAW_ID.as_slice(),
        Status::Running => STATUS_RUNNING_VARIANT_RAW_ID.as_slice(),
    };
    writer.add_enumeration_value(enumeration_id, variant_id);
    writer.add_unit();
}
impl TryFrom<&[u8]> for Status {
    type Error = DeserializationError;
    fn try_from(buffer: &[u8]) -> Result<Self, Self::Error> {
        let mut reader = BufferReader::new(buffer);
        return deserialize_from_reader(&mut reader, true);
    }
}
pub fn deserialize_from_reader(
    reader: &mut BufferReader,
    check_type: bool,
) -> Result<Status, DeserializationError> {
    if check_type {
        let type_raw_id_opt = reader.next_type();
        if type_raw_id_opt.is_none() {
            return Err(DeserializationError {
                message: "missing next type information".to_string(),
            });
        }
        if type_raw_id_opt.unwrap() != TYPE_ENUMERATION {
            return Err(DeserializationError {
                message: "next type is not an enumeration".to_string(),
            });
        }
    }
    if STATUS_ENUM_RAW_ID != reader.get_structure_field() {
        return Err(DeserializationError {
            message: "missing variant information".to_string(),
        });
    }
    let variant_raw_id = reader.get_enumeration_value_raw();
    reader.next_type();
    match variant_raw_id
        .try_into()
        .expect("enum id is of unexpected length")
    {
        STATUS_SUCCESS_VARIANT_RAW_ID => Ok(Status::Success),
        STATUS_FAILURE_VARIANT_RAW_ID => Ok(Status::Failure),
        STATUS_RUNNING_VARIANT_RAW_ID => Ok(Status::Running),
        _ => Err(DeserializationError {
            message: "unexpected variant".to_string(),
        }),
    }
}
impl Into<Value> for Status {
    fn into(self) -> Value {
        match self {
            Status::Success => Value::Enumeration(Enumeration {
                id: Uuid::from_bytes(STATUS_ENUM_RAW_ID),
                variant_id: Uuid::from_bytes(STATUS_SUCCESS_VARIANT_RAW_ID),
                value: Box::new(Value::Unit),
            }),
            Status::Failure => Value::Enumeration(Enumeration {
                id: Uuid::from_bytes(STATUS_ENUM_RAW_ID),
                variant_id: Uuid::from_bytes(STATUS_FAILURE_VARIANT_RAW_ID),
                value: Box::new(Value::Unit),
            }),
            Status::Running => Value::Enumeration(Enumeration {
                id: Uuid::from_bytes(STATUS_ENUM_RAW_ID),
                variant_id: Uuid::from_bytes(STATUS_RUNNING_VARIANT_RAW_ID),
                value: Box::new(Value::Unit),
            }),
        }
    }
}
impl TryFrom<Value> for Status {
    type Error = ConversionError;
    fn try_from(value: Value) -> Result<Self, Self::Error> {
        if let Value::Enumeration(as_enum) = value {
            if *as_enum.id.as_bytes() == STATUS_ENUM_RAW_ID {
                match *as_enum.variant_id.as_bytes() {
                    STATUS_SUCCESS_VARIANT_RAW_ID => Ok(Status::Success),
                    STATUS_FAILURE_VARIANT_RAW_ID => Ok(Status::Failure),
                    STATUS_RUNNING_VARIANT_RAW_ID => Ok(Status::Running),
                    _ => Err(Self::Error {
                        message: "unexpected variant".to_string(),
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
#[doc = "Status: 325a5767-e344-4532-860e-0749bcf2e428"]
pub const STATUS_ENUM_RAW_ID: [u8; 16] = [
    0x32, 0x5a, 0x57, 0x67, 0xe3, 0x44, 0x45, 0x32, 0x86, 0x0e, 0x07, 0x49, 0xbc, 0xf2, 0xe4, 0x28,
];
#[doc = "Status :: Success: 766e9e9a-446d-4e46-83e6-14b7ca101169"]
pub const STATUS_SUCCESS_VARIANT_RAW_ID: [u8; 16] = [
    0x76, 0x6e, 0x9e, 0x9a, 0x44, 0x6d, 0x4e, 0x46, 0x83, 0xe6, 0x14, 0xb7, 0xca, 0x10, 0x11, 0x69,
];
#[doc = "Status :: Failure: 2468f46c-bb60-425c-9a4d-9ad326ccc7e2"]
pub const STATUS_FAILURE_VARIANT_RAW_ID: [u8; 16] = [
    0x24, 0x68, 0xf4, 0x6c, 0xbb, 0x60, 0x42, 0x5c, 0x9a, 0x4d, 0x9a, 0xd3, 0x26, 0xcc, 0xc7, 0xe2,
];
#[doc = "Status :: Running: acd79ec6-0c44-401a-82f8-5da5422d3eec"]
pub const STATUS_RUNNING_VARIANT_RAW_ID: [u8; 16] = [
    0xac, 0xd7, 0x9e, 0xc6, 0x0c, 0x44, 0x40, 0x1a, 0x82, 0xf8, 0x5d, 0xa5, 0x42, 0x2d, 0x3e, 0xec,
];
