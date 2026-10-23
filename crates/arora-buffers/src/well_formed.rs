//! Whether a buffer holds the value it starts with, read as
//! [`froto_value::deserialize`](crate::froto_value::deserialize) reads it, but
//! without panicking. Code handed a buffer it did not write — a guest, given
//! its caller's argument — checks it before decoding it, as the decoder panics
//! on a buffer that is not one, and a guest's panic is a trap.

use crate::{
    ALIGNMENT, BUFFER_SIZE_SIZE, TYPE_ARRAY, TYPE_BOOLEAN, TYPE_ENUMERATION, TYPE_F32, TYPE_F64,
    TYPE_I16, TYPE_I32, TYPE_I64, TYPE_I8, TYPE_MAP, TYPE_OPTION, TYPE_STRING, TYPE_STRUCTURE,
    TYPE_U16, TYPE_U32, TYPE_U64, TYPE_U8, TYPE_UNIT, TYPE_UUID, TYPE_VALUE,
};

/// How deep values may nest. A buffer nesting deeper is refused, as decoding
/// it would recurse as deep.
pub const MAX_DEPTH: usize = 128;

/// Whether `buffer` holds the Arora Buffer it starts with: a size prefix that
/// counts itself and fits in the bytes there are, then a value, within the
/// bytes the prefix counts and nested no deeper than [`MAX_DEPTH`], that
/// [`froto_value::deserialize`](crate::froto_value::deserialize) reads. Says
/// why not.
pub fn well_formed(buffer: &[u8]) -> Result<(), String> {
    let size = buffer
        .get(..BUFFER_SIZE_SIZE)
        .map(|prefix| u32::from_le_bytes(prefix.try_into().expect("4 bytes")) as usize)
        .ok_or("it is shorter than its size prefix")?;
    if !(BUFFER_SIZE_SIZE..=buffer.len()).contains(&size) {
        return Err(format!(
            "its size prefix counts {size} of its {} bytes",
            buffer.len()
        ));
    }
    let mut walk = Walk {
        buffer: &buffer[..size],
        at: BUFFER_SIZE_SIZE,
    };
    walk.value(0)
}

/// A read through a buffer, at a byte offset from its start, as the decoder's
/// reader measures it.
/// Where the value starting at byte `at` of `buffer` ends: the offset of the
/// byte after it, walking it as [`well_formed`] does and decoding nothing.
/// `buffer` is the whole Arora Buffer, its size prefix included, as array
/// alignment counts from its start. Fails as [`well_formed`] does, on a value
/// that runs past `buffer`, nests deeper than [`MAX_DEPTH`] or is not one.
pub fn value_end(buffer: &[u8], at: usize) -> Result<usize, String> {
    let mut walk = Walk { buffer, at };
    walk.value(0)?;
    Ok(walk.at)
}

struct Walk<'a> {
    buffer: &'a [u8],
    at: usize,
}

impl<'a> Walk<'a> {
    fn take(&mut self, len: usize) -> Result<&'a [u8], String> {
        let bytes = self
            .at
            .checked_add(len)
            .and_then(|end| self.buffer.get(self.at..end))
            .ok_or_else(|| format!("a value runs past the end, at byte {}", self.at))?;
        self.at += len;
        Ok(bytes)
    }

    fn skip(&mut self, len: usize) -> Result<(), String> {
        self.take(len).map(drop)
    }

    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<usize, String> {
        let bytes = self.take(4)?.try_into().expect("4 bytes");
        Ok(u32::from_le_bytes(bytes) as usize)
    }

    /// As the reader aligns: to the next multiple of [`ALIGNMENT`], a whole
    /// [`ALIGNMENT`] on from one.
    fn align(&mut self) -> Result<(), String> {
        self.skip(ALIGNMENT - self.at % ALIGNMENT)
    }

    fn string(&mut self) -> Result<(), String> {
        let len = self.u32()?;
        let at = self.at;
        std::str::from_utf8(self.take(len)?)
            .map(drop)
            .map_err(|_| format!("a string is not UTF-8, at byte {at}"))
    }

    /// A structure's fields: each an id, then a value.
    fn fields(&mut self, depth: usize) -> Result<(), String> {
        for _ in 0..self.u32()? {
            self.skip(16)?;
            self.value(depth)?;
        }
        Ok(())
    }

    fn value(&mut self, depth: usize) -> Result<(), String> {
        if depth == MAX_DEPTH {
            return Err(format!("its values nest deeper than {MAX_DEPTH}"));
        }
        let depth = depth + 1;
        let at = self.at;
        match self.u8()? {
            TYPE_UNIT => Ok(()),
            TYPE_BOOLEAN | TYPE_U8 | TYPE_I8 => self.skip(1),
            TYPE_U16 | TYPE_I16 => self.skip(2),
            TYPE_U32 | TYPE_I32 | TYPE_F32 => self.skip(4),
            TYPE_U64 | TYPE_I64 | TYPE_F64 => self.skip(8),
            TYPE_STRING => self.string(),
            TYPE_UUID => self.skip(16),
            TYPE_STRUCTURE => {
                self.skip(16)?;
                self.fields(depth)
            }
            // Its type's id and its variant's, then its value.
            TYPE_ENUMERATION => {
                self.skip(32)?;
                self.value(depth)
            }
            TYPE_OPTION => match self.u8()? {
                0 => Ok(()),
                _ => self.value(depth),
            },
            // Its id, then each field: its key, its id and its value.
            TYPE_MAP => {
                self.skip(16)?;
                for _ in 0..self.u32()? {
                    self.string()?;
                    self.skip(16)?;
                    self.value(depth)?;
                }
                Ok(())
            }
            TYPE_ARRAY => self.array(depth),
            kind => Err(format!("type {kind} at byte {at} is not a value's")),
        }
    }

    /// An array: its elements' type and count, then its elements.
    fn array(&mut self, depth: usize) -> Result<(), String> {
        let ty = self.u8()?;
        let count = self.u32()?;
        let primitive = match ty {
            TYPE_BOOLEAN | TYPE_U8 | TYPE_I8 => Some(1),
            TYPE_U16 | TYPE_I16 => Some(2),
            TYPE_U32 | TYPE_I32 | TYPE_F32 => Some(4),
            TYPE_U64 | TYPE_I64 | TYPE_F64 => Some(8),
            _ => None,
        };
        if let Some(size) = primitive {
            self.align()?;
            let len = count
                .checked_mul(size)
                .ok_or("an array is longer than a buffer can be")?;
            return self.skip(len);
        }
        match ty {
            TYPE_STRING => (0..count).try_for_each(|_| self.string()),
            // The structures' id, then each one's fields.
            TYPE_STRUCTURE => {
                self.skip(16)?;
                (0..count).try_for_each(|_| self.fields(depth))
            }
            // The enumeration's id, then each one's variant and value.
            TYPE_ENUMERATION => {
                self.skip(16)?;
                (0..count).try_for_each(|_| {
                    self.skip(16)?;
                    self.value(depth)
                })
            }
            TYPE_VALUE => (0..count).try_for_each(|_| self.value(depth)),
            ty => Err(format!("an array of type {ty} is not a value's")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::froto_value::{deserialize, serialize};
    use crate::BufferWriter;
    use arora_types::keyvalue::{KeyValue, KeyValueField};
    use arora_types::value::{
        Enumeration, EnumerationWithoutId, Structure, StructureField, StructureWithoutId, Value,
    };
    use uuid::Uuid;

    fn id(n: u8) -> Uuid {
        Uuid::from_bytes([n; 16])
    }

    /// A value of every form the decoder reads.
    fn values() -> Vec<Value> {
        let field = |n, value| StructureField {
            id: id(n),
            value: Box::new(value),
        };
        let mut map = KeyValue::new_with_id(id(9));
        map.set_field(KeyValueField::new_with_id_and_option(
            "key",
            id(10),
            Some(Value::U16(7)),
        ));
        vec![
            Value::Unit,
            Value::Boolean(true),
            Value::I64(-3),
            Value::F32(1.5),
            Value::String("héllo".into()),
            Value::Uuid(id(1)),
            Value::Option(Some(Box::new(Value::U8(1)))),
            Value::Option(None),
            Value::KeyValue(map),
            Value::Structure(Structure {
                id: id(2),
                fields: vec![
                    field(3, Value::U32(4)),
                    field(4, Value::ArrayU8(vec![1, 2, 3])),
                    field(5, Value::ArrayF64(vec![0.5, 2.0])),
                    field(6, Value::ArrayString(vec!["a".into(), "".into()])),
                    field(
                        7,
                        Value::Enumeration(Enumeration {
                            id: id(5),
                            variant_id: id(6),
                            value: Box::new(Value::I16(-1)),
                        }),
                    ),
                ],
            }),
            Value::ArrayStructure {
                id: id(7),
                elements: vec![
                    StructureWithoutId {
                        fields: vec![field(8, Value::Boolean(false))],
                    },
                    StructureWithoutId { fields: vec![] },
                ],
            },
            Value::ArrayEnumeration {
                id: id(8),
                elements: vec![EnumerationWithoutId {
                    variant_id: id(9),
                    value: Box::new(Value::Unit),
                }],
            },
            Value::ArrayValue(vec![Value::U64(1), Value::String("b".into())]),
            Value::ArrayBoolean(vec![]),
        ]
    }

    #[test]
    fn a_buffer_the_writer_wrote_is_well_formed_and_reads_back() {
        for value in values() {
            let buffer = serialize(&value);
            assert_eq!(well_formed(&buffer), Ok(()), "{value:?}");
            assert_eq!(deserialize(&buffer), value);
        }
    }

    /// `value_end` steps over a value to the next: over each field of a
    /// structure holding a value of every form, to the end of the buffer.
    #[test]
    fn value_end_steps_over_a_value_of_every_form() {
        let fields: Vec<StructureField> = values()
            .into_iter()
            .enumerate()
            .map(|(n, value)| StructureField {
                id: id(n as u8),
                value: Box::new(value),
            })
            .collect();
        let count = fields.len();
        let buffer = serialize(&Value::Structure(Structure {
            id: id(200),
            fields,
        }));
        // The structure's tag, id and field count, then each field's id and value.
        let mut at = BUFFER_SIZE_SIZE + 1 + 16 + 4;
        for n in 0..count {
            assert_eq!(&buffer[at..at + 16], id(n as u8).as_bytes(), "field {n}");
            at = value_end(&buffer, at + 16).expect("a value of the structure");
        }
        assert_eq!(at, buffer.len());
        assert_eq!(value_end(&buffer, BUFFER_SIZE_SIZE), Ok(buffer.len()));
    }

    #[test]
    fn a_truncated_buffer_is_refused() {
        for value in values() {
            let buffer = serialize(&value);
            for len in 0..buffer.len() {
                let mut truncated = buffer[..len].to_vec();
                if len >= 4 {
                    truncated[..4].copy_from_slice(&(len as u32).to_le_bytes());
                }
                assert!(well_formed(&truncated).is_err(), "{value:?} cut to {len}");
            }
        }
    }

    /// Whatever a buffer's bytes, one this accepts decodes without a panic.
    #[test]
    fn a_buffer_accepted_decodes_whatever_its_bytes() {
        let hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let mut panicked = Vec::new();
        for value in values() {
            let buffer = serialize(&value).into_vec();
            for at in 4..buffer.len() {
                for byte in [0, 1, 3, 13, 15, 16, 19, 20, 0x7f, 0x80, 0xff] {
                    let mut changed = buffer.clone();
                    changed[at] = byte;
                    if well_formed(&changed).is_ok()
                        && std::panic::catch_unwind(|| deserialize(&changed)).is_err()
                    {
                        panicked.push((value.clone(), at, byte));
                    }
                }
            }
        }
        std::panic::set_hook(hook);
        assert!(panicked.is_empty(), "accepted, then panicked: {panicked:?}");
    }

    #[test]
    fn a_size_prefix_not_counting_the_buffer_is_refused() {
        let buffer = serialize(&Value::U32(1)).into_vec();
        for size in [0u32, 3, buffer.len() as u32 + 1] {
            let mut changed = buffer.clone();
            changed[..4].copy_from_slice(&size.to_le_bytes());
            assert!(well_formed(&changed).is_err(), "{size}");
        }
        assert!(well_formed(&[]).is_err());
        assert!(well_formed(&[8, 0]).is_err());
    }

    #[test]
    fn values_nested_past_the_limit_are_refused() {
        let nested = |depth: usize| {
            let mut value = Value::Unit;
            for _ in 1..depth {
                value = Value::Option(Some(Box::new(value)));
            }
            serialize(&value)
        };
        assert_eq!(well_formed(&nested(MAX_DEPTH)), Ok(()));
        assert!(well_formed(&nested(MAX_DEPTH + 1))
            .unwrap_err()
            .contains("nest deeper"));
    }

    #[test]
    fn an_error_buffer_or_a_string_not_utf_8_is_refused() {
        let mut writer = BufferWriter::new();
        writer.add_error("refused");
        assert!(well_formed(&writer.finalize()).is_err());

        let mut buffer = serialize(&Value::String("ab".into())).into_vec();
        let last = buffer.len() - 1;
        buffer[last] = 0xff;
        assert!(well_formed(&buffer).unwrap_err().contains("not UTF-8"));
    }
}
