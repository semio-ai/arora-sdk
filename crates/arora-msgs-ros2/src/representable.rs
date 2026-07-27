//! Whether an arora type can travel as a ROS 2 message.
//!
//! The ROS 2 bridge exposes a device's keys as topics. A key whose value type
//! the CDR codec cannot carry — a map, an option, an enumeration, or an array of
//! those — is not a ROS message, so the bridge skips it (or falls back to a JSON
//! string). [`ros2_representable`] is that decision, made against the type alone:
//! it accepts exactly the shapes [`crate::cdr`] round-trips and (with the
//! `interop` feature) [`crate::hash`] can hash, so a type that passes here has a
//! faithful place on the ROS 2 wire.

use arora_types::module::low::TypeRef;
use arora_types::ty::{self, low, TypeRegistry};
use arora_types::Uuid;

/// Why an arora type has no ROS 2 message form.
#[derive(Debug, Clone, thiserror::Error)]
#[error("not representable as a ROS 2 message: {reason}")]
pub struct NotRepresentable {
    pub reason: String,
}

fn no(reason: impl Into<String>) -> NotRepresentable {
    NotRepresentable {
        reason: reason.into(),
    }
}

/// Whether `ty` (and every type it nests, resolved through `registry`) can be
/// encoded as a ROS 2 message. `Ok(())` means [`crate::cdr::encode`] will accept
/// a matching value and, under `interop`, [`crate::hash::rihs01`] will produce a
/// hash. An `Err` names the first unrepresentable shape.
pub fn ros2_representable(ty: &low::Type, registry: &TypeRegistry) -> Result<(), NotRepresentable> {
    let mut seen = std::collections::HashSet::from([ty.id]);
    check_type(ty, registry, &mut seen)
}

fn check_type(
    ty: &low::Type,
    registry: &TypeRegistry,
    seen: &mut std::collections::HashSet<Uuid>,
) -> Result<(), NotRepresentable> {
    match &ty.kind {
        low::TypeKind::Structure(structure) => {
            for field in structure.fields.values() {
                check_ref(&field.type_ref, registry, seen)
                    .map_err(|e| no(format!("field `{}`: {}", field.name, e.reason)))?;
            }
            Ok(())
        }
        // A bare primitive/array message (a topic of one scalar or one sequence).
        low::TypeKind::Primitive(type_ref) => check_ref(type_ref, registry, seen),
        low::TypeKind::Enumeration(_) => Err(no(
            "enumerations have no ROS 2 CDR encoding (ROS models these as integer constants)",
        )),
    }
}

fn check_ref(
    type_ref: &TypeRef,
    registry: &TypeRegistry,
    seen: &mut std::collections::HashSet<Uuid>,
) -> Result<(), NotRepresentable> {
    match type_ref {
        TypeRef::Scalar { id } => check_element(id, registry, seen),
        // A ROS sequence (`T[]`) or fixed array (`T[N]`): its element is a scalar
        // or a nested message.
        TypeRef::Array { id } => check_element(id, registry, seen),
        TypeRef::FixedArray { id, .. } => check_element(id, registry, seen),
        TypeRef::Map { .. } => Err(no("key/value maps have no ROS 2 CDR encoding")),
    }
}

/// A scalar id, or a nested type id resolved (and recursed) through `registry`.
fn check_element(
    id: &Uuid,
    registry: &TypeRegistry,
    seen: &mut std::collections::HashSet<Uuid>,
) -> Result<(), NotRepresentable> {
    if is_ros_scalar(id) {
        return Ok(());
    }
    let nested = registry.get(id).ok_or_else(|| {
        no(format!(
            "type {id} is neither a ROS scalar nor present in the registry"
        ))
    })?;
    // A message struct can (indirectly) reach itself only through a cycle, which
    // ROS .msg cannot express; the guard keeps a malformed registry from looping.
    if !seen.insert(*id) {
        return Ok(());
    }
    check_type(nested, registry, seen)
}

/// The ROS 2 primitive field types: `bool`, the sized ints, the floats, and
/// `string`. Deliberately excludes arora's `unit`, `uuid` and `option` — none is
/// a ROS field type. This is the scalar set [`crate::cdr`] and REP-2016 share.
pub fn is_ros_scalar(id: &Uuid) -> bool {
    let id = *id;
    id == *ty::BOOLEAN_ID
        || id == *ty::I8_ID
        || id == *ty::I16_ID
        || id == *ty::I32_ID
        || id == *ty::I64_ID
        || id == *ty::U8_ID
        || id == *ty::U16_ID
        || id == *ty::U32_ID
        || id == *ty::U64_ID
        || id == *ty::F32_ID
        || id == *ty::F64_ID
        || id == *ty::STRING_ID
}

#[cfg(test)]
mod tests {
    use super::*;
    use arora_types::ty::low::{Structure, StructureField, Type, TypeKind};

    fn id(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    fn structure(name: &str, type_id: Uuid, fields: Vec<(Uuid, StructureField)>) -> Type {
        Type {
            name: name.to_string(),
            id: type_id,
            description: String::new(),
            kind: TypeKind::Structure(Structure::from_fields(fields)),
        }
    }

    fn field(name: &str, type_ref: TypeRef) -> StructureField {
        StructureField {
            name: name.to_string(),
            type_ref,
        }
    }

    #[test]
    fn a_struct_of_scalars_strings_and_sequences_is_representable() {
        // scalars + string + a nested struct + a sequence of that struct.
        let point = structure(
            "geometry_msgs/msg/Point",
            id(1),
            vec![
                (id(11), field("x", TypeRef::Scalar { id: *ty::F64_ID })),
                (id(12), field("y", TypeRef::Scalar { id: *ty::F64_ID })),
            ],
        );
        let path = structure(
            "geometry_msgs/msg/Polyline",
            id(2),
            vec![
                (
                    id(21),
                    field("frame", TypeRef::Scalar { id: *ty::STRING_ID }),
                ),
                (id(22), field("stamp", TypeRef::Scalar { id: *ty::U32_ID })),
                (id(23), field("points", TypeRef::Array { id: id(1) })),
            ],
        );
        let mut registry = TypeRegistry::new();
        registry.insert(point.id, point);
        registry.insert(path.id, path.clone());
        assert!(ros2_representable(&path, &registry).is_ok());
    }

    #[test]
    fn a_map_field_is_not_representable() {
        let bad = structure(
            "my_msgs/msg/Bad",
            id(3),
            vec![(
                id(31),
                field(
                    "meta",
                    TypeRef::Map {
                        key_id: *ty::STRING_ID,
                        value_id: *ty::STRING_ID,
                    },
                ),
            )],
        );
        let mut registry = TypeRegistry::new();
        registry.insert(bad.id, bad.clone());
        let err = ros2_representable(&bad, &registry).unwrap_err();
        assert!(err.reason.contains("meta"), "reason names the field");
    }

    #[test]
    fn an_enumeration_type_is_not_representable() {
        let e = Type {
            name: "my_msgs/msg/Mode".to_string(),
            id: id(4),
            description: String::new(),
            kind: TypeKind::Enumeration(arora_types::ty::low::Enumeration {
                values: Default::default(),
            }),
        };
        let registry = TypeRegistry::new();
        assert!(ros2_representable(&e, &registry).is_err());
    }

    #[test]
    fn a_nested_type_missing_from_the_registry_is_rejected() {
        let outer = structure(
            "my_msgs/msg/Outer",
            id(5),
            vec![(id(51), field("inner", TypeRef::Scalar { id: id(999) }))],
        );
        let mut registry = TypeRegistry::new();
        registry.insert(outer.id, outer.clone());
        assert!(ros2_representable(&outer, &registry).is_err());
    }
}
