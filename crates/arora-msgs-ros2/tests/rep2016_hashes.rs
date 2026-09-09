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
//! a `uint8[]` sequence.

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
