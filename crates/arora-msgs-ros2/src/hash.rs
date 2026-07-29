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
//! Structures of scalars, strings, nested structures and homogeneous arrays
//! (ROS sequences `T[]` and fixed arrays `T[N]`) are supported — the same shape
//! the CDR codec ([`crate::cdr`]) carries. Maps have no ROS 2 form and are
//! rejected.

use std::collections::HashSet;

use arora_types::module::low::TypeRef;
use arora_types::ty::{low, TypeRegistry};
use arora_types::Uuid;
use ros2_client::type_description::{
    type_id as rid, Field, FieldType, IndividualTypeDescription, TypeDescription,
};

use crate::representable::is_ros_scalar;

/// Failure to derive a ROS type description from an arora type.
#[derive(Debug, thiserror::Error)]
#[error("ros type hash: {0}")]
pub struct Error(String);

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

/// The array shape of a field: a single value, a fixed `[N]`, or an unbounded
/// `[]` sequence.
enum Shape {
    Unit,
    Fixed(u64),
    Unbounded,
}

/// The REP-2016 `FieldType` for one field's arora type reference: a base scalar
/// or a nested (named) type, in its array shape.
fn field_type(type_ref: &TypeRef, registry: &TypeRegistry) -> Result<FieldType, Error> {
    let (id, shape) = match type_ref {
        TypeRef::Scalar { id } => (id, Shape::Unit),
        TypeRef::Array { id } => (id, Shape::Unbounded),
        TypeRef::FixedArray { id, len } => (id, Shape::Fixed(*len as u64)),
        TypeRef::Map { .. } => {
            return Err(Error("key/value maps have no REP-2016 field type".into()))
        }
        TypeRef::Option { .. } => {
            return Err(Error("optional values have no REP-2016 field type".into()))
        }
    };
    if let Some(base) = rep2016_scalar(id) {
        Ok(scalar_field(base, shape))
    } else if let Some(nested) = registry.get(id) {
        Ok(nested_field(ros_name(nested)?, shape))
    } else {
        Err(Error(format!(
            "type id {id} is neither a known scalar nor in the registry"
        )))
    }
}

/// A scalar base in its array shape. REP-2016 encodes the shape as an offset on
/// the base type id: `+ARRAY_OFFSET` for a fixed array, `+UNBOUNDED_SEQUENCE_OFFSET`
/// for a sequence.
fn scalar_field(base: u8, shape: Shape) -> FieldType {
    match shape {
        Shape::Unit => FieldType::scalar(base),
        Shape::Fixed(capacity) => FieldType::array(base, capacity),
        Shape::Unbounded => FieldType {
            type_id: base + rid::UNBOUNDED_SEQUENCE_OFFSET,
            capacity: 0,
            string_capacity: 0,
            nested_type_name: String::new(),
        },
    }
}

/// A nested (named) type in its array shape.
fn nested_field(name: String, shape: Shape) -> FieldType {
    match shape {
        Shape::Unit => FieldType::nested(name),
        Shape::Fixed(capacity) => FieldType {
            type_id: rid::NESTED_TYPE + rid::ARRAY_OFFSET,
            capacity,
            string_capacity: 0,
            nested_type_name: name,
        },
        Shape::Unbounded => FieldType {
            type_id: rid::NESTED_TYPE + rid::UNBOUNDED_SEQUENCE_OFFSET,
            capacity: 0,
            string_capacity: 0,
            nested_type_name: name,
        },
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
        // The element type of a scalar, sequence or fixed array; a map has none.
        let id = match &field.type_ref {
            TypeRef::Scalar { id } | TypeRef::Array { id } | TypeRef::FixedArray { id, .. } => *id,
            // Maps and options have no ROS field form, so they are not walked.
            TypeRef::Map { .. } | TypeRef::Option { .. } => continue,
        };
        if rep2016_scalar(&id).is_some() || !seen.insert(id) {
            continue;
        }
        let nested = registry
            .get(&id)
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
/// or `None` if `id` is not a ROS scalar (a nested type or an unsupported ref).
/// The recognised set is [`is_ros_scalar`]'s, in REP-2016 order.
fn rep2016_scalar(id: &Uuid) -> Option<u8> {
    use arora_types::ty;
    if !is_ros_scalar(id) {
        return None;
    }
    let id = *id;
    Some(if id == *ty::BOOLEAN_ID {
        rid::BOOLEAN
    } else if id == *ty::I8_ID {
        rid::INT8
    } else if id == *ty::I16_ID {
        rid::INT16
    } else if id == *ty::I32_ID {
        rid::INT32
    } else if id == *ty::I64_ID {
        rid::INT64
    } else if id == *ty::U8_ID {
        rid::UINT8
    } else if id == *ty::U16_ID {
        rid::UINT16
    } else if id == *ty::U32_ID {
        rid::UINT32
    } else if id == *ty::U64_ID {
        rid::UINT64
    } else if id == *ty::F32_ID {
        rid::FLOAT
    } else if id == *ty::F64_ID {
        rid::DOUBLE
    } else {
        rid::STRING
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use arora_types::ty;

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

    fn unbounded(base: u8) -> FieldType {
        FieldType {
            type_id: base + rid::UNBOUNDED_SEQUENCE_OFFSET,
            capacity: 0,
            string_capacity: 0,
            nested_type_name: String::new(),
        }
    }

    // A scalar sequence, a fixed array, and a sequence of a nested type: the
    // arora-derived hash matches the direct REP-2016 construction, proving the
    // offset math (`+48` fixed array, `+144` unbounded sequence) is right.
    #[test]
    fn array_fields_hash_like_the_direct_type_description() {
        let point = message(
            "geometry_msgs/msg/Point",
            id(0x33),
            vec![
                (id(0x331), field("x", *ty::F64_ID)),
                (id(0x332), field("y", *ty::F64_ID)),
                (id(0x333), field("z", *ty::F64_ID)),
            ],
        );
        let array_field = |name: &str, type_ref| low::StructureField {
            name: name.to_string(),
            type_ref,
        };
        let shape = low::Type {
            name: "my_msgs/msg/Shape".to_string(),
            id: id(0x55),
            description: String::new(),
            kind: low::TypeKind::Structure(low::Structure::from_fields(vec![
                (
                    id(0x551),
                    array_field("weights", TypeRef::Array { id: *ty::F64_ID }),
                ),
                (
                    id(0x552),
                    array_field(
                        "matrix",
                        TypeRef::FixedArray {
                            id: *ty::F64_ID,
                            len: 9,
                        },
                    ),
                ),
                (
                    id(0x553),
                    array_field("points", TypeRef::Array { id: id(0x33) }),
                ),
            ])),
        };
        let mut registry = TypeRegistry::new();
        registry.insert(point.id, point);
        registry.insert(shape.id, shape.clone());
        let from_arora = rihs01(&shape, &registry).unwrap();

        let point_desc = IndividualTypeDescription::new(
            "geometry_msgs/msg/Point",
            vec![
                Field::new("x", FieldType::scalar(rid::DOUBLE)),
                Field::new("y", FieldType::scalar(rid::DOUBLE)),
                Field::new("z", FieldType::scalar(rid::DOUBLE)),
            ],
        );
        let nested_sequence = FieldType {
            type_id: rid::NESTED_TYPE + rid::UNBOUNDED_SEQUENCE_OFFSET,
            capacity: 0,
            string_capacity: 0,
            nested_type_name: "geometry_msgs/msg/Point".into(),
        };
        let top = IndividualTypeDescription::new(
            "my_msgs/msg/Shape",
            vec![
                Field::new("weights", unbounded(rid::DOUBLE)),
                Field::new("matrix", FieldType::array(rid::DOUBLE, 9)),
                Field::new("points", nested_sequence),
            ],
        );
        let direct = TypeDescription::new(top, vec![point_desc]).rihs01();
        assert_eq!(from_arora, direct);
    }

    #[test]
    fn missing_ros_name_is_rejected() {
        let anon = message("", id(0x55), vec![(id(0x551), field("x", *ty::F64_ID))]);
        let mut registry = TypeRegistry::new();
        registry.insert(anon.id, anon.clone());
        assert!(rihs01(&anon, &registry).is_err());
    }
}
