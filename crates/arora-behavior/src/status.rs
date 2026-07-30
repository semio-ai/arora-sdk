//! The behavior `Status` — the cross-interpreter run-status contract.
//!
//! `Status` is the value a task run writes to its status key and the value the
//! ROS 2 action plane maps to a goal status. It lives here, in `arora-behavior`
//! (beside [`BehaviorStatus`](crate::BehaviorStatus), `TaskHandle`, and
//! `RunPolicy`), rather than in any single interpreter's crate: a node graph
//! and a behavior tree both speak it, and neither should depend on the other's
//! types for it.
//!
//! The Rust type is the source of truth for the schema — it derives
//! [`AroraType`](arora_types::AroraType), pinning the enumeration id and each
//! variant id, so no hand-authored record or codegen is needed. The value-plane
//! conversions ([`From<Status>`](Value)/[`TryFrom<Value>`]) produce the
//! `Value::Enumeration { id, variant_id, Unit }` form the plane already speaks;
//! their ids match the derived schema, so the two never drift.

use arora_types::value::{ConversionError, Enumeration, Value};
use arora_types::AroraType;
use arora_types::Uuid;

/// A run's lifecycle status: still running, or a terminal outcome.
///
/// The one status vocabulary a behavior speaks on the value plane — the value
/// an interpreter writes to a run's status key, and the value the ROS 2 action
/// plane maps to `GoalStatus`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, AroraType)]
#[arora(id = "325a5767-e344-4532-860e-0749bcf2e428")]
pub enum Status {
    /// The behavior finished cleanly.
    #[arora(id = "766e9e9a-446d-4e46-83e6-14b7ca101169")]
    Success,
    /// The behavior finished in failure (also what a halted run reports — a
    /// halted run did not reach its goal).
    #[arora(id = "2468f46c-bb60-425c-9a4d-9ad326ccc7e2")]
    Failure,
    /// The behavior is still running; tick it again.
    #[arora(id = "acd79ec6-0c44-401a-82f8-5da5422d3eec")]
    Running,
}

/// The enumeration type id — the action-shape marker the ROS 2 bridge keys off,
/// and the id every `Status` value carries. Matches the derived schema
/// ([`Status::arora_type_id`](arora_types::AroraType::arora_type_id)).
pub const STATUS_ENUMERATION_ID: Uuid = Uuid::from_bytes([
    0x32, 0x5a, 0x57, 0x67, 0xe3, 0x44, 0x45, 0x32, 0x86, 0x0e, 0x07, 0x49, 0xbc, 0xf2, 0xe4, 0x28,
]);
/// The `Status::Success` variant id.
pub const STATUS_SUCCESS_VARIANT_ID: Uuid = Uuid::from_bytes([
    0x76, 0x6e, 0x9e, 0x9a, 0x44, 0x6d, 0x4e, 0x46, 0x83, 0xe6, 0x14, 0xb7, 0xca, 0x10, 0x11, 0x69,
]);
/// The `Status::Failure` variant id.
pub const STATUS_FAILURE_VARIANT_ID: Uuid = Uuid::from_bytes([
    0x24, 0x68, 0xf4, 0x6c, 0xbb, 0x60, 0x42, 0x5c, 0x9a, 0x4d, 0x9a, 0xd3, 0x26, 0xcc, 0xc7, 0xe2,
]);
/// The `Status::Running` variant id.
pub const STATUS_RUNNING_VARIANT_ID: Uuid = Uuid::from_bytes([
    0xac, 0xd7, 0x9e, 0xc6, 0x0c, 0x44, 0x40, 0x1a, 0x82, 0xf8, 0x5d, 0xa5, 0x42, 0x2d, 0x3e, 0xec,
]);
/// The version the `Status` type is registered under.
pub const STATUS_ENUMERATION_VERSION: semver::Version = semver::Version::new(1, 0, 0);

impl From<Status> for Value {
    fn from(status: Status) -> Value {
        let variant_id = match status {
            Status::Success => STATUS_SUCCESS_VARIANT_ID,
            Status::Failure => STATUS_FAILURE_VARIANT_ID,
            Status::Running => STATUS_RUNNING_VARIANT_ID,
        };
        Value::Enumeration(Enumeration {
            id: STATUS_ENUMERATION_ID,
            variant_id,
            value: Box::new(Value::Unit),
        })
    }
}

impl TryFrom<Value> for Status {
    type Error = ConversionError;
    fn try_from(value: Value) -> Result<Self, Self::Error> {
        let Value::Enumeration(enumeration) = value else {
            return Err(ConversionError {
                message: "expected an enumeration value for Status".to_string(),
            });
        };
        if enumeration.id != STATUS_ENUMERATION_ID {
            return Err(ConversionError {
                message: "enumeration id is not Status".to_string(),
            });
        }
        match enumeration.variant_id {
            STATUS_SUCCESS_VARIANT_ID => Ok(Status::Success),
            STATUS_FAILURE_VARIANT_ID => Ok(Status::Failure),
            STATUS_RUNNING_VARIANT_ID => Ok(Status::Running),
            _ => Err(ConversionError {
                message: "unknown Status variant".to_string(),
            }),
        }
    }
}

/// Declare the `Status` enumeration in the record/registry form — the
/// hand-authored declaration the behavior-tree and message codegen consume, kept
/// alongside the type so the two stay in one place. The derived schema
/// ([`Status::arora_type`](arora_types::AroraType::arora_type)) is the same type
/// in `ty::low` form.
pub fn declare_status_enumeration(
    parent: Uuid,
) -> arora_types::record::enumeration::unfrozen::Enumeration {
    use arora_types::record::enumeration::unfrozen::{Enumeration, EnumerationVariant};
    use arora_types::record::ty::{Primitive, PrimitiveKind, UnfrozenTy};
    let unit = || {
        UnfrozenTy::Primitive(Primitive {
            kind: PrimitiveKind::Unit,
        })
    };
    Enumeration {
        name: "Status".to_string(),
        parent,
        variants: [
            (
                STATUS_SUCCESS_VARIANT_ID,
                EnumerationVariant {
                    name: "Success".to_string(),
                    ty: unit(),
                },
            ),
            (
                STATUS_FAILURE_VARIANT_ID,
                EnumerationVariant {
                    name: "Failure".to_string(),
                    ty: unit(),
                },
            ),
            (
                STATUS_RUNNING_VARIANT_ID,
                EnumerationVariant {
                    name: "Running".to_string(),
                    ty: unit(),
                },
            ),
        ]
        .into_iter()
        .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The derived schema and the pinned const ids agree — the id the value
    /// plane carries is the id the schema declares.
    #[test]
    fn derived_schema_matches_the_pinned_ids() {
        assert_eq!(Status::arora_type_id(), STATUS_ENUMERATION_ID);
        let arora_types::ty::low::TypeKind::Enumeration(enumeration) = Status::arora_type().kind
        else {
            panic!("Status is an enumeration");
        };
        let ids: Vec<_> = enumeration.values.keys().copied().collect();
        assert_eq!(
            ids,
            vec![
                STATUS_SUCCESS_VARIANT_ID,
                STATUS_FAILURE_VARIANT_ID,
                STATUS_RUNNING_VARIANT_ID
            ]
        );
    }

    /// The value-plane conversions round-trip and produce the stable
    /// `Value::Enumeration { STATUS_ENUMERATION_ID, variant, Unit }` form.
    #[test]
    fn status_round_trips_through_value() {
        for status in [Status::Success, Status::Failure, Status::Running] {
            let value: Value = status.into();
            let Value::Enumeration(ref enumeration) = value else {
                panic!("a Status is an enumeration value");
            };
            assert_eq!(enumeration.id, STATUS_ENUMERATION_ID);
            assert_eq!(*enumeration.value, Value::Unit);
            assert_eq!(Status::try_from(value).unwrap(), status);
        }
    }
}
