use crate::arora_generated::error::DeserializationError;
use arora_buffers::*;
use uuid::Uuid;
pub struct TickId {
    pub callable_id: u64,
}
impl Into<Box<[u8]>> for TickId {
    fn into(self) -> Box<[u8]> {
        let mut writer = BufferWriter::new();
        serialize_to_writer(&self, &mut writer);
        writer.finalize()
    }
}
pub fn serialize_to_writer(value: &TickId, writer: &mut BufferWriter) {
    let structure_id = TICK_ID_STRUCT_RAW_ID.as_slice();
    writer.begin_structure(structure_id, 1u32);
    writer.add_structure_field(&TICK_ID_CALLABLE_ID_FIELD_RAW_ID);
    writer.add_u64(value.callable_id)
}
impl TryFrom<&[u8]> for TickId {
    type Error = DeserializationError;
    fn try_from(buffer: &[u8]) -> Result<Self, Self::Error> {
        let mut reader = BufferReader::new(buffer);
        return deserialize_from_reader(&mut reader, true);
    }
}
pub fn deserialize_from_reader(
    reader: &mut BufferReader,
    check_type: bool,
) -> Result<TickId, DeserializationError> {
    let field_count = if check_type {
        let type_raw_id_opt = reader.next_type();
        if type_raw_id_opt.is_none() {
            return Err(DeserializationError {
                message: "missing next type information".to_string(),
            });
        }
        if type_raw_id_opt.unwrap() != TYPE_STRUCTURE {
            return Err(DeserializationError {
                message: "next type is not a structure".to_string(),
            });
        }
        let (structure_raw_id, field_count) = reader.get_structure();
        if TICK_ID_STRUCT_RAW_ID != structure_raw_id {
            return Err(DeserializationError {
                message: "structure id does not match".to_string(),
            });
        }
        field_count
    } else {
        reader.get_structure_raw()
    };
    if 1usize != field_count as usize {
        return Err(DeserializationError {
            message: format!("expected {} fields, found {}", 1usize, field_count),
        });
    }
    let mut tick_id_callable_id: Option<u64> = None;
    for _ in 0..field_count {
        let field_raw_id = reader.get_structure_field();
        if field_raw_id == TICK_ID_CALLABLE_ID_FIELD_RAW_ID {
            tick_id_callable_id = Some({
                {
                    let _next_type = reader.next_type();
                    assert_eq!(_next_type, Some(TYPE_U64), "type mismatch");
                }
                reader.get_u64()
            });
        } else {
            return Err(DeserializationError {
                message: format!(
                    "unexpected struct field {}",
                    Uuid::from_slice(field_raw_id).unwrap().to_string()
                ),
            });
        }
    }
    Ok(TickId {
        callable_id: tick_id_callable_id.unwrap(),
    })
}
#[doc = "TickId: 6f49e650-84ca-4899-a9bd-1f3bf17fab51"]
pub const TICK_ID_STRUCT_RAW_ID: [u8; 16] = [
    0x6f, 0x49, 0xe6, 0x50, 0x84, 0xca, 0x48, 0x99, 0xa9, 0xbd, 0x1f, 0x3b, 0xf1, 0x7f, 0xab, 0x51,
];
#[doc = "TickId :: CallableId: 237992d2-17d1-459f-bca1-7185fa6a69d7"]
pub const TICK_ID_CALLABLE_ID_FIELD_RAW_ID: [u8; 16] = [
    0x23, 0x79, 0x92, 0xd2, 0x17, 0xd1, 0x45, 0x9f, 0xbc, 0xa1, 0x71, 0x85, 0xfa, 0x6a, 0x69, 0xd7,
];
