//! The five typical uses of `arora-msgs-ros2`, as runnable tests and worked
//! examples. Each mirrors one of the scenarios the crate is built for.

use arora_msgs_ros2::{
    builtin_interfaces, cdr, geometry_msgs, ros2_representable, sensor_msgs, std_msgs, Ros2Registry,
};
use arora_types::module::low::TypeRef;
use arora_types::ty::low::{Structure, StructureField as LowField, Type, TypeKind};
use arora_types::value::{Structure as ValueStructure, StructureField, Value};
use arora_types::value_serde::bridge::to_value_seeded;
use arora_types::{gen_uuid_from_str, ty, AroraType, Uuid};

/// A `Value` structure field keyed the way a message type keys it — the field
/// name hashed, matching the generated types and the runtime `define` path.
fn field(name: &str, value: Value) -> StructureField {
    StructureField {
        id: gen_uuid_from_str(name),
        value: Box::new(value),
    }
}

// ── Use case 1 ──────────────────────────────────────────────────────────────
// "I am the bridge and I recognize the Value is not ROS-supported, so I skip it."
#[test]
fn use_case_1_bridge_skips_a_non_ros_value() {
    let registry = arora_msgs_ros2::registry();

    // A bundled message type rides ROS 2 fine.
    let point = <geometry_msgs::Point as AroraType>::arora_type();
    assert!(ros2_representable(&point, registry.types()).is_ok());

    // A type with a key/value map has no ROS 2 CDR form — the bridge skips it
    // (or falls back to a JSON string), rather than emitting an unreadable topic.
    let with_map = Type {
        name: "my_msgs/msg/Bag".into(),
        id: Uuid::from_u128(0xB16),
        description: String::new(),
        kind: TypeKind::Structure(Structure::from_fields(vec![(
            Uuid::from_u128(0xF1),
            LowField {
                name: "meta".into(),
                type_ref: TypeRef::Map {
                    key_id: *ty::STRING_ID,
                    value_id: *ty::STRING_ID,
                },
            },
        )])),
    };
    assert!(ros2_representable(&with_map, registry.types()).is_err());
}

// ── Use case 2 ──────────────────────────────────────────────────────────────
// "I am a ROS developer with a new type; add it to the registry without adding
// the message file to the crate."
#[test]
fn use_case_2_define_a_new_type_at_runtime() {
    let mut registry = arora_msgs_ros2::registry();

    // A brand-new message that reuses the bundled std_msgs/Header — no recompile.
    let id = registry
        .define_from_msg("my_msgs", "Reading", "std_msgs/Header header\nfloat64 value\n")
        .unwrap();
    assert_eq!(registry.id_of("my_msgs/Reading"), Some(id));

    // It encodes and decodes against its bundled dependency. The Header value is
    // built from the generated struct via the seeded bridge.
    let header = std_msgs::Header {
        stamp: builtin_interfaces::Time { sec: 1, nanosec: 2 },
        frame_id: "map".into(),
    };
    let (header_ty, header_reg) = <std_msgs::Header as AroraType>::arora_type_with_registry();
    let header_value = to_value_seeded(&header, &header_ty, &header_reg).unwrap();

    let ty = registry.get(&id).unwrap().clone();
    let value = Value::Structure(ValueStructure {
        id,
        fields: vec![field("header", header_value), field("value", Value::F64(9.5))],
    });
    let bytes = cdr::encode(&ty, registry.types(), &value).unwrap();
    assert_eq!(cdr::decode(&ty, registry.types(), &bytes).unwrap(), value);
}

// ── Use case 3 ──────────────────────────────────────────────────────────────
// "I am writing a module that receives a ROS message and I want to work with it
// in Rust directly." A generated message is a plain Rust type whose fields you
// hold and mutate; ros2-client's typed pub/sub (de)serializes it over CDR. Here
// we exercise that serde round-trip on sensor_msgs/Imu — nested structs and the
// fixed array float64[9]. (For the dynamic, schema-driven plane, the same
// message travels as a Value through the CDR codec; see use cases 2 and 5.)
#[test]
fn use_case_3_work_with_a_message_in_rust() {
    let imu = sensor_msgs::Imu {
        header: std_msgs::Header {
            stamp: builtin_interfaces::Time { sec: 5, nanosec: 6 },
            frame_id: "imu".into(),
        },
        orientation: geometry_msgs::Quaternion {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            w: 1.0,
        },
        orientation_covariance: [0.0; 9],
        angular_velocity: geometry_msgs::Vector3 {
            x: 1.0,
            y: 2.0,
            z: 3.0,
        },
        angular_velocity_covariance: [0.01; 9],
        linear_acceleration: geometry_msgs::Vector3 {
            x: 4.0,
            y: 5.0,
            z: 6.0,
        },
        linear_acceleration_covariance: [0.02; 9],
    };

    let bytes = serde_json::to_vec(&imu).unwrap();
    let back: sensor_msgs::Imu = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(imu, back);

    // The same type carries its arora type and ROS-qualified name, so it also
    // takes part in the dynamic Value/CDR path the bridge uses.
    assert_eq!(
        <sensor_msgs::Imu as AroraType>::arora_type().name,
        "sensor_msgs/msg/Imu"
    );
}

/// The float64[36] covariance messages (de)serialize via serde-big-array — the
/// derive path serde does not cover for arrays longer than 32.
#[test]
fn a_big_fixed_array_message_serde_round_trips() {
    let pose = geometry_msgs::PoseWithCovariance {
        pose: geometry_msgs::Pose {
            position: geometry_msgs::Point {
                x: 1.0,
                y: 2.0,
                z: 3.0,
            },
            orientation: geometry_msgs::Quaternion {
                x: 0.0,
                y: 0.0,
                z: 0.0,
                w: 1.0,
            },
        },
        covariance: [0.5; 36],
    };
    let json = serde_json::to_string(&pose).unwrap();
    let back: geometry_msgs::PoseWithCovariance = serde_json::from_str(&json).unwrap();
    assert_eq!(pose, back);
}

// ── Use case 4 ──────────────────────────────────────────────────────────────
// "I am using a behavior graph node editor and I want to select the type of a
// key from its name, to publish or read data on it."
#[test]
fn use_case_4_pick_a_type_by_name() {
    let registry = arora_msgs_ros2::registry();

    // The editor offers the known message names.
    let names = registry.names();
    assert!(names.contains(&"geometry_msgs/msg/Point"));
    assert!(names.contains(&"sensor_msgs/msg/Imu"));
    assert!(names.contains(&"hri_msgs/msg/LiveSpeech"));

    // Either name form resolves to the type, which types the key.
    let ty = registry.get_by_name("geometry_msgs/Point").unwrap();
    assert_eq!(ty.name, "geometry_msgs/msg/Point");
    assert_eq!(
        registry.id_of("geometry_msgs/Point"),
        Some(<geometry_msgs::Point as AroraType>::arora_type_id())
    );
}

// ── Use case 5 ──────────────────────────────────────────────────────────────
// "I am using the node editor and I define a new type on the fly (a JSON blob,
// the equivalent of a .msg), save it in my behavior, and using it on a topic
// makes it available to ROS clients (if a ROS bridge is enabled)."
#[test]
fn use_case_5_define_a_type_on_the_fly_and_save_it() {
    // The behavior saved the authored type as JSON — a serialized `low::Type`.
    // (Here we author it from a `.msg`-shaped blob and serialize, to stand in for
    // the editor's output.)
    let saved_json = {
        let mut scratch = Ros2Registry::new();
        let id = scratch
            .define_from_msg("my_msgs", "Blob", "float64 a\nstring b\n")
            .unwrap();
        serde_json::to_string(&vec![scratch.get(&id).unwrap().clone()]).unwrap()
    };

    // On load, the running registry takes the saved type in — no recompile.
    let mut registry = arora_msgs_ros2::registry();
    let ids = registry.define_from_json(&saved_json).unwrap();
    let ty = registry.get(&ids[0]).unwrap().clone();
    assert_eq!(registry.id_of("my_msgs/Blob"), Some(ty.id));

    // Using it on a topic: it CDR-encodes, so an enabled ROS bridge can carry it
    // to real ROS clients.
    let value = Value::Structure(ValueStructure {
        id: ty.id,
        fields: vec![field("a", Value::F64(2.5)), field("b", Value::String("hi".into()))],
    });
    let bytes = cdr::encode(&ty, registry.types(), &value).unwrap();
    assert_eq!(cdr::decode(&ty, registry.types(), &bytes).unwrap(), value);
}

// A bonus, behind the interop feature: the same message reaches native ROS
// nodes because its REP-2016 type hash is computed from the arora type.
#[cfg(feature = "interop")]
#[test]
fn a_bundled_type_has_a_rep2016_hash() {
    let registry = arora_msgs_ros2::registry();
    for name in ["geometry_msgs/Point", "sensor_msgs/Imu", "hri_msgs/Skeleton2D"] {
        let ty = registry.get_by_name(name).unwrap();
        // Imu nests structs and a float64[9]; Skeleton2D has a nested sequence —
        // both hash through the array-aware REP-2016 mapping.
        let hash = arora_msgs_ros2::rihs01(ty, registry.types()).unwrap();
        assert!(
            hash.starts_with("RIHS01_") && hash.len() == 71,
            "unexpected hash for {name}: {hash}"
        );
    }
}
