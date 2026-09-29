//! The hashes ROS 2 itself computes for its message types, and ours agreeing
//! with them.
//!
//! `rmw_zenoh` keys every publisher on its message's REP-2016 hash, and a native
//! subscriber listens on that exact key: a hash that is merely well-formed
//! reaches nothing. So the values below are not derived here — they are read
//! from the type-description files a ROS 2 Jazzy install ships
//! (`share/<pkg>/msg/<Type>.json`, `type_hashes[].hash_string`), the same
//! source `rclpy` announces on the wire. Both image messages exercise what a
//! scalar cannot: a nested `Header` with its own nested `Time`, a `string`, and
//! a `uint8[]` sequence. `rcl_interfaces/msg/ParameterDescriptor` is ROS 2's own
//! spelling of optional fields, `FloatingPointRange[<=1]`: described with arora
//! optionals, it hashes as ROS 2 does.

#![cfg(feature = "interop")]

/// `(registry name, RIHS01 from ROS 2 Jazzy)`.
const AUTHORITATIVE: &[(&str, &str)] = &[
    (
        "sensor_msgs/CompressedImage",
        "RIHS01_15640771531571185e2efc8a100baf923961a4d15d5569652e6cb6691e8e371a",
    ),
    (
        "sensor_msgs/Image",
        "RIHS01_d31d41a9a4c4bc8eae9be757b0beed306564f7526c88ea6a4588fb9582527d47",
    ),
    (
        "std_msgs/Header",
        "RIHS01_f49fb3ae2cf070f793645ff749683ac6b06203e41c891e17701b1cb597ce6a01",
    ),
    (
        "builtin_interfaces/Time",
        "RIHS01_b106235e25a4c5ed35098aa0a61a3ee9c9b18d197f398b0e4206cea9acf9c197",
    ),
    (
        "std_msgs/Float64",
        "RIHS01_705ba9c3d1a09df43737eb67095534de36fd426c0587779bda2bc51fe790182a",
    ),
];

#[test]
fn bundled_types_hash_exactly_as_ros2_does() {
    let registry = arora_msgs_ros2::registry();
    for (name, expected) in AUTHORITATIVE {
        let ty = registry
            .get_by_name(name)
            .unwrap_or_else(|| panic!("{name} is a bundled message"));
        let hash = arora_msgs_ros2::rihs01(ty, registry.types())
            .unwrap_or_else(|e| panic!("{name} does not hash: {e}"));
        assert_eq!(
            &hash, expected,
            "{name}: a native rmw_zenoh subscriber listens on the right-hand key; ours would \
             publish on the left"
        );
    }
}

/// `rcl_interfaces/msg/ParameterDescriptor` from ROS 2 Jazzy
/// (`share/rcl_interfaces/msg/ParameterDescriptor.json`).
const PARAMETER_DESCRIPTOR: &str =
    "RIHS01_52175dbfda6c51153101d33d2a9da05743f66f02d5ab2ca9ec4709b46b73d704";

#[test]
fn an_optional_hashes_as_the_bounded_sequence_ros2_declares() {
    use arora_types::module::low::TypeRef;
    use arora_types::ty::{self, low, TypeRegistry};
    use arora_types::{gen_uuid_from_str, Uuid};

    let message = |name: &str, fields: &[(&str, TypeRef)]| low::Type {
        name: name.to_string(),
        id: gen_uuid_from_str(name),
        description: String::new(),
        kind: low::TypeKind::Structure(low::Structure::from_fields(fields.iter().map(
            |(field, type_ref)| {
                (
                    gen_uuid_from_str(&format!("{name}.{field}")),
                    low::StructureField {
                        name: field.to_string(),
                        type_ref: type_ref.clone(),
                    },
                )
            },
        ))),
    };
    let scalar = |id: &Uuid| TypeRef::Scalar { id: *id };
    let floating_point_range = message(
        "rcl_interfaces/msg/FloatingPointRange",
        &[
            ("from_value", scalar(&ty::F64_ID)),
            ("to_value", scalar(&ty::F64_ID)),
            ("step", scalar(&ty::F64_ID)),
        ],
    );
    let integer_range = message(
        "rcl_interfaces/msg/IntegerRange",
        &[
            ("from_value", scalar(&ty::I64_ID)),
            ("to_value", scalar(&ty::I64_ID)),
            ("step", scalar(&ty::U64_ID)),
        ],
    );
    let descriptor = message(
        "rcl_interfaces/msg/ParameterDescriptor",
        &[
            ("name", scalar(&ty::STRING_ID)),
            ("type", scalar(&ty::U8_ID)),
            ("description", scalar(&ty::STRING_ID)),
            ("additional_constraints", scalar(&ty::STRING_ID)),
            ("read_only", scalar(&ty::BOOLEAN_ID)),
            ("dynamic_typing", scalar(&ty::BOOLEAN_ID)),
            (
                "floating_point_range",
                TypeRef::Option {
                    id: floating_point_range.id,
                },
            ),
            (
                "integer_range",
                TypeRef::Option {
                    id: integer_range.id,
                },
            ),
        ],
    );
    let mut registry = TypeRegistry::new();
    for ty in [floating_point_range, integer_range, descriptor.clone()] {
        registry.insert(ty.id, ty);
    }
    assert_eq!(
        arora_msgs_ros2::rihs01(&descriptor, &registry).unwrap(),
        PARAMETER_DESCRIPTOR
    );
}
