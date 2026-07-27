//! Compile-time ROS 2 identity for the generated message types.

/// A generated ROS 2 message: its ROS identity, known at compile time, on top of
/// the [`AroraType`](arora_types::AroraType) schema. Every struct in the
/// generated `std_msgs` / `geometry_msgs` / `sensor_msgs` / … modules implements
/// it, so a consumer names a topic's type without stringly-typed lookups:
/// `MessageTypeName::new(T::PACKAGE, T::TYPE_NAME)`.
pub trait RosMessage: arora_types::AroraType {
    /// The REP-2016 qualified name, e.g. `"sensor_msgs/msg/JointState"`.
    const ROS_TYPE_NAME: &'static str;
    /// The ROS package, e.g. `"sensor_msgs"`.
    const PACKAGE: &'static str;
    /// The message type name, e.g. `"JointState"`.
    const TYPE_NAME: &'static str;
}

/// The REP-2016 type hash (`RIHS01_…`) of a message type — what a Zenoh
/// publisher keys on to reach native (C++) ROS nodes. Computed from the type
/// alone, so a caller writes `type_hash::<sensor_msgs::JointState>()`.
///
/// The `interop` feature (needs `ros2-client`); on DDS the hash is not required.
#[cfg(feature = "interop")]
pub fn type_hash<T: RosMessage>() -> Result<String, crate::hash::Error> {
    let (ty, registry) = <T as arora_types::AroraType>::arora_type_with_registry();
    crate::hash::rihs01(&ty, &registry)
}
