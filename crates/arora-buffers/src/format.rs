//! The arora-buffers wire-format constants: the type tags, the alignment, and
//! the size prefix that define the byte layout. See the crate README for the
//! full spec.
//!
//! Every value on the wire is a 1-byte type tag followed by its payload. A
//! buffer opens with a little-endian `u32` total size (the prefix included) and
//! is otherwise packed — the only alignment is the bulk body of a primitive
//! array, padded to [`ALIGNMENT`] measured from the buffer start.

/// unit — tag only.
pub const TYPE_UNIT: u8 = 0;
/// bool — 1 byte, `0`/`1`.
pub const TYPE_BOOLEAN: u8 = 1;
/// `u8` — 1 byte.
pub const TYPE_U8: u8 = 2;
/// `u16` — 2 bytes LE.
pub const TYPE_U16: u8 = 3;
/// `u32` — 4 bytes LE.
pub const TYPE_U32: u8 = 4;
/// `u64` — 8 bytes LE.
pub const TYPE_U64: u8 = 5;
/// `i8` — 1 byte.
pub const TYPE_I8: u8 = 6;
/// `i16` — 2 bytes LE.
pub const TYPE_I16: u8 = 7;
/// `i32` — 4 bytes LE.
pub const TYPE_I32: u8 = 8;
/// `i64` — 8 bytes LE.
pub const TYPE_I64: u8 = 9;
/// `f32` — 4 bytes LE.
pub const TYPE_F32: u8 = 10;
/// `f64` — 8 bytes LE.
pub const TYPE_F64: u8 = 11;
/// string — `u32` byte length + UTF-8 bytes (no NUL).
pub const TYPE_STRING: u8 = 12;
/// structure — 16-byte type id + `u32` field count + `[field id: 16][value]`×.
pub const TYPE_STRUCTURE: u8 = 13;
/// enumeration — 16-byte type id + 16-byte variant id + value.
pub const TYPE_ENUMERATION: u8 = 14;
/// array — element tag + `u32` count + elements (the element type is tagged once).
pub const TYPE_ARRAY: u8 = 15;
/// map (keyvalue) — 16-byte id + `u32` count + `[key_len: u32][key][field id: 16][value]`×.
pub const TYPE_MAP: u8 = 16;
/// option — 1 presence byte (`0`/`1`); the value follows if present.
pub const TYPE_OPTION: u8 = 17;
/// uuid — 16 bytes.
pub const TYPE_UUID: u8 = 18;
/// dynamic value — the element tag of a heterogeneous, self-describing array.
pub const TYPE_VALUE: u8 = 19;
/// error — `u32` length + UTF-8 message.
pub const TYPE_ERROR: u8 = 20;

/// Alignment of the one aligned region (a primitive array's bulk body), measured
/// from the buffer start (the size prefix included), so the block can be read as
/// a slice.
pub const ALIGNMENT: usize = 8;

/// Size of the leading `u32` that announces the total buffer length.
pub const BUFFER_SIZE_SIZE: usize = std::mem::size_of::<u32>();
