//! `TickId` — the handle a behavior-tree control node receives for each child
//! and ticks it through (`arora_dispatch_indirect`).

use arora_types::record::{
    structure::unfrozen::{Structure, StructureField},
    ty::{Primitive, PrimitiveKind, UnfrozenTy},
};
use arora_types::AroraType;
use semver::Version;
use uuid::Uuid;

use arora_types::record::scope::ARORA;

/// A child a control node ticks: the callable the tree registered for it.
///
/// The Rust type of the `arora.TickId` record — it derives [`AroraType`]
/// under the record's ids, so its value-plane conversions are the record's
/// `Value::Structure` form, and its record lives in the [`ARORA`] scope
/// whatever names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, AroraType)]
#[arora(id = "6f49e650-84ca-4899-a9bd-1f3bf17fab51", scope = ARORA)]
pub struct TickId {
    /// The callable that ticks the child.
    #[arora(id = "237992d2-17d1-459f-bca1-7185fa6a69d7")]
    pub callable_id: u64,
}

/// Declare the `TickId` structure in the record/registry form, for a registry
/// that resolves a `module.yaml` naming `arora.TickId`. The derived
/// schema ([`TickId::arora_type`]) is the same type in `ty::low` form.
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
        )]
        .into_iter()
        .collect(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use arora_types::value::{Structure as ValueStructure, StructureField as ValueField, Value};

    /// The derived type carries the record's ids, so its value is the
    /// record's structure form.
    #[test]
    fn tick_id_is_the_record_structure() {
        assert_eq!(TickId::arora_type_id(), TICK_ID_STRUCTURE_ID);
        let value: Value = TickId { callable_id: 7 }.into();
        assert_eq!(
            value,
            Value::Structure(ValueStructure {
                id: TICK_ID_STRUCTURE_ID,
                fields: vec![ValueField {
                    id: TICK_ID_CALLABLE_ID_FIELD_RAW_ID,
                    value: Box::new(Value::U64(7)),
                }],
            })
        );
        assert_eq!(TickId::try_from(value).unwrap(), TickId { callable_id: 7 });
        let declared = declare_tick_id_structure(Uuid::nil());
        let ids: Vec<_> = declared.fields.keys().copied().collect();
        assert_eq!(ids, vec![TICK_ID_CALLABLE_ID_FIELD_RAW_ID]);
    }
}
