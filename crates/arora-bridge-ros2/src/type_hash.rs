//! REP-2016 type hashing (`RIHS01_…`) for a message described in the Arora
//! realm.
//!
//! `rmw_zenoh` keys every entity with the message's type hash, and native (C++)
//! subscribers match only the concrete hash. [`rihs01`] reproduces, from a
//! runtime [`low::Type`], the exact hash a C++ peer computes from the equivalent
//! `.msg` — so an arora publisher keyed with it (via `ros2_client`'s
//! `create_publisher_with_type_hash`) reaches native subscribers, with no
//! compiled IDL and no per-type Rust.
//!
//! The type and every type it nests must carry its ROS-qualified name
//! (`geometry_msgs/msg/PointStamped`, `builtin_interfaces/msg/Time`, …) in
//! [`low::Type::name`], because REP-2016 hashes those names alongside the fields.
//! Only structures of scalars, strings and nested structures are supported so
//! far — the same shape the CDR codec ([`crate::cdr`]) carries; arrays/sequences
//! and maps are rejected until the walk grows them.

use std::collections::HashSet;

use arora_types::module::low::TypeRef;
use arora_types::ty::{self, low, TypeRegistry};
use arora_types::Uuid;
use ros2_client::type_description::{
    type_id as rid, Field, FieldType, IndividualTypeDescription, TypeDescription,
};

/// Failure to derive a ROS type description from an arora type.
#[derive(Debug)]
pub struct Error(String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ros type hash: {}", self.0)
    }
}
impl std::error::Error for Error {}

/// The REP-2016 type hash (`RIHS01_…`) of the ROS 2 message described by arora
/// type `ty` and its dependency `registry`.
pub fn rihs01(ty: &low::Type, registry: &TypeRegistry) -> Result<String, Error> {
    let top = individual(ty, registry)?;
    let mut referenced = Vec::new();
    let mut seen = HashSet::from([ty.id]);
    collect_referenced(ty, registry, &mut referenced, &mut seen)?;
    Ok(TypeDescription::new(top, referenced).rihs01())
}

/// The `IndividualTypeDescription` for one structure type — its ROS name and its
/// fields, without recursing into nested types.
fn individual(ty: &low::Type, registry: &TypeRegistry) -> Result<IndividualTypeDescription, Error> {
    let low::TypeKind::Structure(structure) = &ty.kind else {
        return Err(Error(format!(
            "type {:?} is not a structure (only message structs map to REP-2016)",
            ros_name(ty)?
        )));
    };
    let mut fields = Vec::with_capacity(structure.fields.len());
    for field in structure.fields.values() {
        fields.push(Field::new(
            field.name.clone(),
            field_type(&field.type_ref, registry)?,
        ));
    }
    Ok(IndividualTypeDescription::new(ros_name(ty)?, fields))
}

/// The REP-2016 `FieldType` for one field's arora type reference: a base scalar,
/// or a nested (named) type resolved through the registry.
fn field_type(type_ref: &TypeRef, registry: &TypeRegistry) -> Result<FieldType, Error> {
    match type_ref {
        TypeRef::Scalar { id } => {
            if let Some(tid) = rep2016_scalar(id) {
                Ok(FieldType::scalar(tid))
            } else if let Some(nested) = registry.get(id) {
                Ok(FieldType::nested(ros_name(nested)?))
            } else {
                Err(Error(format!(
                    "type id {id} is neither a known scalar nor in the registry"
                )))
            }
        }
        // Arrays/sequences and maps have REP-2016 encodings, but the arora walk
        // does not carry them yet (see the arrays follow-up); reject explicitly.
        other => Err(Error(format!("unsupported field type {other:?}"))),
    }
}

/// Append the `IndividualTypeDescription` of every type transitively nested
/// under `ty` (each once) to `out`. Ordering is irrelevant — REP-2016 hashing
/// sorts referenced descriptions by name.
fn collect_referenced(
    ty: &low::Type,
    registry: &TypeRegistry,
    out: &mut Vec<IndividualTypeDescription>,
    seen: &mut HashSet<Uuid>,
) -> Result<(), Error> {
    let low::TypeKind::Structure(structure) = &ty.kind else {
        return Ok(());
    };
    for field in structure.fields.values() {
        let TypeRef::Scalar { id } = &field.type_ref else {
            continue;
        };
        if rep2016_scalar(id).is_some() || !seen.insert(*id) {
            continue;
        }
        let nested = registry
            .get(id)
            .ok_or_else(|| Error(format!("nested type id {id} not in the registry")))?;
        out.push(individual(nested, registry)?);
        collect_referenced(nested, registry, out, seen)?;
    }
    Ok(())
}

fn ros_name(ty: &low::Type) -> Result<String, Error> {
    if ty.name.is_empty() {
        return Err(Error(format!(
            "type id {} has no ROS-qualified name (e.g. geometry_msgs/msg/Point)",
            ty.id
        )));
    }
    Ok(ty.name.clone())
}

/// Map an arora well-known scalar type id to its REP-2016 `FieldType` type id,
/// or `None` if `id` is not a scalar (a nested type or an unsupported ref).
fn rep2016_scalar(id: &Uuid) -> Option<u8> {
    let id = *id;
    if id == *ty::BOOLEAN_ID {
        Some(rid::BOOLEAN)
    } else if id == *ty::I8_ID {
        Some(rid::INT8)
    } else if id == *ty::I16_ID {
        Some(rid::INT16)
    } else if id == *ty::I32_ID {
        Some(rid::INT32)
    } else if id == *ty::I64_ID {
        Some(rid::INT64)
    } else if id == *ty::U8_ID {
        Some(rid::UINT8)
    } else if id == *ty::U16_ID {
        Some(rid::UINT16)
    } else if id == *ty::U32_ID {
        Some(rid::UINT32)
    } else if id == *ty::U64_ID {
        Some(rid::UINT64)
    } else if id == *ty::F32_ID {
        Some(rid::FLOAT)
    } else if id == *ty::F64_ID {
        Some(rid::DOUBLE)
    } else if id == *ty::STRING_ID {
        Some(rid::STRING)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    fn field(name: &str, type_id: Uuid) -> low::StructureField {
        low::StructureField {
            name: name.to_string(),
            type_ref: TypeRef::Scalar { id: type_id },
        }
    }

    fn message(
        ros_name: &str,
        type_id: Uuid,
        fields: Vec<(Uuid, low::StructureField)>,
    ) -> low::Type {
        low::Type {
            name: ros_name.to_string(),
            id: type_id,
            description: String::new(),
            kind: low::TypeKind::Structure(low::Structure::from_fields(fields)),
        }
    }

    // geometry_msgs/PointStamped, described in the Arora realm with ROS names.
    fn point_stamped() -> (low::Type, TypeRegistry) {
        let time = message(
            "builtin_interfaces/msg/Time",
            id(0x11),
            vec![
                (id(0x111), field("sec", *ty::I32_ID)),
                (id(0x112), field("nanosec", *ty::U32_ID)),
            ],
        );
        let header = message(
            "std_msgs/msg/Header",
            id(0x22),
            vec![
                (id(0x221), field("stamp", id(0x11))),
                (id(0x222), field("frame_id", *ty::STRING_ID)),
            ],
        );
        let point = message(
            "geometry_msgs/msg/Point",
            id(0x33),
            vec![
                (id(0x331), field("x", *ty::F64_ID)),
                (id(0x332), field("y", *ty::F64_ID)),
                (id(0x333), field("z", *ty::F64_ID)),
            ],
        );
        let point_stamped = message(
            "geometry_msgs/msg/PointStamped",
            id(0x44),
            vec![
                (id(0x441), field("header", id(0x22))),
                (id(0x442), field("point", id(0x33))),
            ],
        );
        let mut registry = TypeRegistry::new();
        for t in [time, header, point, point_stamped.clone()] {
            registry.insert(t.id, t);
        }
        (point_stamped, registry)
    }

    // The hash derived from the arora type equals the one built directly the
    // ros2_client way — proving the arora -> REP-2016 mapping is faithful. (The
    // description itself is the real geometry_msgs/PointStamped; the live probe
    // proves that native rmw_zenoh agrees.)
    #[test]
    fn point_stamped_hash_matches_direct_type_description() {
        let (ty, registry) = point_stamped();
        let from_arora = rihs01(&ty, &registry).expect("hash");

        let time = IndividualTypeDescription::new(
            "builtin_interfaces/msg/Time",
            vec![
                Field::new("sec", FieldType::scalar(rid::INT32)),
                Field::new("nanosec", FieldType::scalar(rid::UINT32)),
            ],
        );
        let header = IndividualTypeDescription::new(
            "std_msgs/msg/Header",
            vec![
                Field::new("stamp", FieldType::nested("builtin_interfaces/msg/Time")),
                Field::new("frame_id", FieldType::scalar(rid::STRING)),
            ],
        );
        let point = IndividualTypeDescription::new(
            "geometry_msgs/msg/Point",
            vec![
                Field::new("x", FieldType::scalar(rid::DOUBLE)),
                Field::new("y", FieldType::scalar(rid::DOUBLE)),
                Field::new("z", FieldType::scalar(rid::DOUBLE)),
            ],
        );
        let top = IndividualTypeDescription::new(
            "geometry_msgs/msg/PointStamped",
            vec![
                Field::new("header", FieldType::nested("std_msgs/msg/Header")),
                Field::new("point", FieldType::nested("geometry_msgs/msg/Point")),
            ],
        );
        let direct = TypeDescription::new(top, vec![header, time, point]).rihs01();

        assert_eq!(
            from_arora, direct,
            "arora-derived hash must match the direct one"
        );
        assert!(from_arora.starts_with("RIHS01_") && from_arora.len() == 71);
    }

    #[test]
    fn missing_ros_name_is_rejected() {
        let anon = message("", id(0x55), vec![(id(0x551), field("x", *ty::F64_ID))]);
        let mut registry = TypeRegistry::new();
        registry.insert(anon.id, anon.clone());
        assert!(rihs01(&anon, &registry).is_err());
    }
}
