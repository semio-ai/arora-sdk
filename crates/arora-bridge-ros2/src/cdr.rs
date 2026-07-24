//! ROS 2 CDR (XCDR1) as an [`arora_types::value_serde::walk`] backend.
//!
//! [`CdrWriter`]/[`CdrReader`] implement [`ValueWriter`]/[`ValueReader`] over an
//! OMG-CDR little-endian byte stream, so [`write_value`]/[`read_value`] can
//! (de)serialize any [`Value`] against a runtime [`low::Type`] — no Rust struct
//! per ROS message type, and no JSON fallback.
//!
//! CDR is positional and not self-describing: there is no struct id, field id or
//! field count on the wire, so the walk's struct framing (`begin_struct`/
//! `begin_field`, `enter_struct`/`enter_field`) is a no-op here — the type drives
//! the shape. Each primitive of size N is aligned to N against the payload start,
//! and the alignment origin resets to 0 *after* the 4-byte encapsulation header
//! (the rmw_cyclonedds / rmw_fastrtps convention). Strings: align 4, u32 length
//! *including* the NUL terminator, bytes, then the NUL.
//!
//! First cut mirrors the walk: scalars, string and nested structures. Arrays
//! (ROS sequences), enumerations and options follow when the walk's type model
//! grows them.

use arora_types::ty::{low, TypeRegistry};
use arora_types::value::Value;
use arora_types::value_serde::walk::{read_value, write_value, ValueReader, ValueWriter};
use arora_types::value_serde::{Error, Result};
use arora_types::Uuid;

/// CDR little-endian encapsulation header (`representation_identifier` +
/// `options`).
const CDR_LE_HEADER: [u8; 4] = [0x00, 0x01, 0x00, 0x00];

/// Serialize `value` to a ROS 2 CDR payload against `ty`.
pub fn encode(ty: &low::Type, registry: &TypeRegistry, value: &Value) -> Result<Vec<u8>> {
    let mut writer = CdrWriter::new();
    write_value(ty, registry, value, &mut writer)?;
    let mut out = CDR_LE_HEADER.to_vec();
    out.extend_from_slice(&writer.body);
    Ok(out)
}

/// Deserialize a ROS 2 CDR payload to a [`Value`] against `ty`.
pub fn decode(ty: &low::Type, registry: &TypeRegistry, bytes: &[u8]) -> Result<Value> {
    if bytes.len() < 4 {
        return Err(Error::new(
            "CDR payload shorter than the 4-byte encapsulation header",
        ));
    }
    // Little-endian (CDR_LE) only for now; big-endian is the same code path with
    // from_be_bytes and a swapped header check.
    if bytes[0..2] != CDR_LE_HEADER[0..2] {
        return Err(Error::new(format!(
            "unsupported CDR encapsulation {:02x} {:02x} (little-endian only)",
            bytes[0], bytes[1]
        )));
    }
    let mut reader = CdrReader::new(&bytes[4..]);
    read_value(ty, registry, &mut reader)
}

/// Accumulates a CDR payload body. Alignment is measured against `body.len()`,
/// i.e. the payload start — the origin resets after the encapsulation header.
pub struct CdrWriter {
    body: Vec<u8>,
}

impl CdrWriter {
    pub fn new() -> Self {
        Self { body: Vec::new() }
    }

    fn align(&mut self, n: usize) {
        while self.body.len() % n != 0 {
            self.body.push(0);
        }
    }

    fn put(&mut self, bytes: &[u8]) {
        self.body.extend_from_slice(bytes);
    }
}

impl Default for CdrWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl ValueWriter for CdrWriter {
    fn write_unit(&mut self) -> Result<()> {
        Ok(())
    }
    fn write_bool(&mut self, v: bool) -> Result<()> {
        self.body.push(v as u8);
        Ok(())
    }
    fn write_u8(&mut self, v: u8) -> Result<()> {
        self.body.push(v);
        Ok(())
    }
    fn write_i8(&mut self, v: i8) -> Result<()> {
        self.body.push(v as u8);
        Ok(())
    }
    fn write_u16(&mut self, v: u16) -> Result<()> {
        self.align(2);
        self.put(&v.to_le_bytes());
        Ok(())
    }
    fn write_i16(&mut self, v: i16) -> Result<()> {
        self.align(2);
        self.put(&v.to_le_bytes());
        Ok(())
    }
    fn write_u32(&mut self, v: u32) -> Result<()> {
        self.align(4);
        self.put(&v.to_le_bytes());
        Ok(())
    }
    fn write_i32(&mut self, v: i32) -> Result<()> {
        self.align(4);
        self.put(&v.to_le_bytes());
        Ok(())
    }
    fn write_f32(&mut self, v: f32) -> Result<()> {
        self.align(4);
        self.put(&v.to_le_bytes());
        Ok(())
    }
    fn write_u64(&mut self, v: u64) -> Result<()> {
        self.align(8);
        self.put(&v.to_le_bytes());
        Ok(())
    }
    fn write_i64(&mut self, v: i64) -> Result<()> {
        self.align(8);
        self.put(&v.to_le_bytes());
        Ok(())
    }
    fn write_f64(&mut self, v: f64) -> Result<()> {
        self.align(8);
        self.put(&v.to_le_bytes());
        Ok(())
    }
    fn write_string(&mut self, v: &str) -> Result<()> {
        self.align(4);
        let bytes = v.as_bytes();
        // Length includes the NUL terminator.
        self.put(&((bytes.len() + 1) as u32).to_le_bytes());
        self.put(bytes);
        self.body.push(0);
        Ok(())
    }
    fn begin_struct(&mut self, _id: Uuid, _field_count: usize) -> Result<()> {
        Ok(())
    }
    fn begin_field(&mut self, _id: Uuid) -> Result<()> {
        Ok(())
    }
}

/// Reads a CDR payload body. Alignment is measured against `pos` — the payload
/// start, the reader having been given the bytes after the encapsulation header.
pub struct CdrReader<'a> {
    body: &'a [u8],
    pos: usize,
}

impl<'a> CdrReader<'a> {
    pub fn new(body: &'a [u8]) -> Self {
        Self { body, pos: 0 }
    }

    fn align(&mut self, n: usize) {
        while self.pos % n != 0 {
            self.pos += 1;
        }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        if self.pos + n > self.body.len() {
            return Err(Error::new(format!(
                "CDR buffer underrun: need {n} bytes at {}, have {}",
                self.pos,
                self.body.len()
            )));
        }
        let s = &self.body[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }

    fn take_aligned<const N: usize>(&mut self, align: usize) -> Result<[u8; N]> {
        self.align(align);
        let mut out = [0u8; N];
        out.copy_from_slice(self.take(N)?);
        Ok(out)
    }
}

impl ValueReader for CdrReader<'_> {
    fn read_unit(&mut self) -> Result<()> {
        Ok(())
    }
    fn read_bool(&mut self) -> Result<bool> {
        Ok(self.take(1)?[0] != 0)
    }
    fn read_u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn read_i8(&mut self) -> Result<i8> {
        Ok(self.take(1)?[0] as i8)
    }
    fn read_u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take_aligned::<2>(2)?))
    }
    fn read_i16(&mut self) -> Result<i16> {
        Ok(i16::from_le_bytes(self.take_aligned::<2>(2)?))
    }
    fn read_u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take_aligned::<4>(4)?))
    }
    fn read_i32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(self.take_aligned::<4>(4)?))
    }
    fn read_f32(&mut self) -> Result<f32> {
        Ok(f32::from_le_bytes(self.take_aligned::<4>(4)?))
    }
    fn read_u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.take_aligned::<8>(8)?))
    }
    fn read_i64(&mut self) -> Result<i64> {
        Ok(i64::from_le_bytes(self.take_aligned::<8>(8)?))
    }
    fn read_f64(&mut self) -> Result<f64> {
        Ok(f64::from_le_bytes(self.take_aligned::<8>(8)?))
    }
    fn read_string(&mut self) -> Result<String> {
        let len = self.read_u32()? as usize;
        if len == 0 {
            return Err(Error::new(
                "CDR string length 0 (must include the NUL terminator)",
            ));
        }
        let bytes = self.take(len)?;
        // Drop the trailing NUL.
        std::str::from_utf8(&bytes[..len - 1])
            .map(|s| s.to_string())
            .map_err(|e| Error::new(format!("invalid utf-8 in CDR string: {e}")))
    }
    fn enter_struct(&mut self, _expected_id: Uuid, _field_count: usize) -> Result<()> {
        Ok(())
    }
    fn enter_field(&mut self, _expected_id: Uuid) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arora_types::module::low::TypeRef;
    use arora_types::ty;
    use arora_types::value::{Structure, StructureField};

    fn id(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    fn scalar(name: &str, type_id: Uuid) -> low::StructureField {
        low::StructureField {
            name: name.to_string(),
            type_ref: TypeRef::Scalar { id: type_id },
        }
    }

    fn structure(type_id: Uuid, fields: Vec<(Uuid, low::StructureField)>) -> low::Type {
        low::Type {
            name: String::new(),
            id: type_id,
            description: String::new(),
            kind: low::TypeKind::Structure(low::Structure::from_fields(fields)),
        }
    }

    // builtin_interfaces/Time { int32 sec; uint32 nanosec }
    fn time_ty() -> low::Type {
        structure(
            id(0x11),
            vec![
                (id(0x111), scalar("sec", *ty::I32_ID)),
                (id(0x112), scalar("nanosec", *ty::U32_ID)),
            ],
        )
    }
    // std_msgs/Header { Time stamp; string frame_id }
    fn header_ty() -> low::Type {
        structure(
            id(0x22),
            vec![
                (id(0x221), scalar("stamp", id(0x11))),
                (id(0x222), scalar("frame_id", *ty::STRING_ID)),
            ],
        )
    }
    // geometry_msgs/Point { float64 x, y, z }
    fn point_ty() -> low::Type {
        structure(
            id(0x33),
            vec![
                (id(0x331), scalar("x", *ty::F64_ID)),
                (id(0x332), scalar("y", *ty::F64_ID)),
                (id(0x333), scalar("z", *ty::F64_ID)),
            ],
        )
    }
    // geometry_msgs/PointStamped { Header header; Point point }
    fn point_stamped_ty() -> low::Type {
        structure(
            id(0x44),
            vec![
                (id(0x441), scalar("header", id(0x22))),
                (id(0x442), scalar("point", id(0x33))),
            ],
        )
    }

    fn registry() -> TypeRegistry {
        let mut r = TypeRegistry::new();
        for t in [time_ty(), header_ty(), point_ty(), point_stamped_ty()] {
            r.insert(t.id, t);
        }
        r
    }

    fn st(type_id: u128, fields: Vec<StructureField>) -> Value {
        Value::Structure(Structure {
            id: id(type_id),
            fields,
        })
    }
    fn f(field_id: u128, v: Value) -> StructureField {
        StructureField {
            id: id(field_id),
            value: Box::new(v),
        }
    }

    fn point_stamped_value(frame: &str) -> Value {
        st(
            0x44,
            vec![
                f(
                    0x441,
                    st(
                        0x22,
                        vec![
                            f(
                                0x221,
                                st(
                                    0x11,
                                    vec![f(0x111, Value::I32(1)), f(0x112, Value::U32(2))],
                                ),
                            ),
                            f(0x222, Value::String(frame.to_string())),
                        ],
                    ),
                ),
                f(
                    0x442,
                    st(
                        0x33,
                        vec![
                            f(0x331, Value::F64(1.0)),
                            f(0x332, Value::F64(2.0)),
                            f(0x333, Value::F64(3.0)),
                        ],
                    ),
                ),
            ],
        )
    }

    #[test]
    fn point_stamped_round_trips() {
        let ty = point_stamped_ty();
        let registry = registry();
        for frame in ["map", "odom", "", "a_long_frame_name"] {
            let value = point_stamped_value(frame);
            let bytes = encode(&ty, &registry, &value).unwrap();
            let back = decode(&ty, &registry, &bytes).unwrap();
            assert_eq!(value, back, "round-trip mismatch for frame {frame:?}");
        }
    }

    /// The load-bearing test: the exact CDR bytes ROS 2 (rmw over CDR_LE) puts on
    /// the wire for a known PointStamped, computed by hand from the CDR rules —
    /// matching them is a standalone interop proof. frame_id = "odom" (len 5 incl
    /// NUL) forces 7 bytes of padding before the 8-aligned doubles.
    #[test]
    fn point_stamped_matches_golden_cdr() {
        let ty = point_stamped_ty();
        let registry = registry();
        let bytes = encode(&ty, &registry, &point_stamped_value("odom")).unwrap();

        #[rustfmt::skip]
        let golden: Vec<u8> = vec![
            0x00, 0x01, 0x00, 0x00,             // encapsulation: CDR_LE
            0x01, 0x00, 0x00, 0x00,             // Time.sec = 1 (i32)
            0x02, 0x00, 0x00, 0x00,             // Time.nanosec = 2 (u32)
            0x05, 0x00, 0x00, 0x00,             // frame_id length = 5 (incl NUL)
            0x6F, 0x64, 0x6F, 0x6D, 0x00,       // "odom\0"
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // pad to 8-align (7 bytes)
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xF0, 0x3F, // Point.x = 1.0 (f64 LE)
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40, // Point.y = 2.0
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x08, 0x40, // Point.z = 3.0
        ];
        assert_eq!(
            bytes, golden,
            "CDR bytes diverge from the hand-computed wire form"
        );
        assert_eq!(
            decode(&ty, &registry, &golden).unwrap(),
            point_stamped_value("odom")
        );
    }

    #[test]
    fn a_value_not_matching_the_type_is_rejected() {
        let ty = point_ty();
        let registry = registry();
        // z declared f64, given an i32.
        let bad = st(
            0x33,
            vec![
                f(0x331, Value::F64(1.0)),
                f(0x332, Value::F64(2.0)),
                f(0x333, Value::I32(3)),
            ],
        );
        assert!(encode(&ty, &registry, &bad).is_err());
    }
}
