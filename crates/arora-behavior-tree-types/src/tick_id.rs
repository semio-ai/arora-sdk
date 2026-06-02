use semio_record::{
  acl::Acl,
  structure::v0::unfrozen::{Structure, StructureField},
  ty::{Primitive, PrimitiveKind, UnfrozenTy},
};
use semver::Version;
use uuid::Uuid;

pub fn declare_tick_id_structure(parent: Uuid) -> Structure {
  Structure {
    name: "TickId".to_string(),
    parent,
    fields: [(
      TICK_ID_CALLABLE_ID_FIELD_RAW_ID,
      StructureField {
        name: "callable_id".to_string(),
        ty: UnfrozenTy::Primitive(Primitive {
          kind: PrimitiveKind::U64,
        }),
      },
    )].into_iter().collect(),
    acl: <Acl as Default>::default(),
  }
}

/// Use this ID to register the type to a registry.
pub const TICK_ID_STRUCTURE_ID: Uuid = Uuid::from_bytes([
  0x6f, 0x49, 0xe6, 0x50, 0x84, 0xca, 0x48, 0x99, 0xa9, 0xbd, 0x1f, 0x3b, 0xf1, 0x7f, 0xab, 0x51,
]);
pub const TICK_ID_CALLABLE_ID_FIELD_RAW_ID: Uuid = Uuid::from_bytes([
  0x23, 0x79, 0x92, 0xd2, 0x17, 0xd1, 0x45, 0x9f, 0xbc, 0xa1, 0x71, 0x85, 0xfa, 0x6a, 0x69, 0xd7,
]);
pub const TICK_ID_STRUCTURE_VERSION: Version = Version::new(1, 0, 0);
