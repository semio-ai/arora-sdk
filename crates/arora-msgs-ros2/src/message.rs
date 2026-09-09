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

#[cfg(test)]
mod tests {
    use super::RosMessage;
    use crate::sensor_msgs::Image;
    use crate::trajectory_msgs::JointTrajectory;
    use arora_types::AroraType;

    /// The strings the generated identity is made of, pinned once: every other
    /// caller reaches a message's type through `T::ROS_TYPE_NAME` (or its id)
    /// rather than repeating a name, so this is where a change of form —
    /// `sensor_msgs/msg/Image` against `sensor_msgs/Image` — would show.
    #[test]
    fn generated_messages_carry_their_ros_name() {
        assert_eq!(Image::ROS_TYPE_NAME, "sensor_msgs/msg/Image");
        assert_eq!(Image::PACKAGE, "sensor_msgs");
        assert_eq!(Image::TYPE_NAME, "Image");
        assert_eq!(
            JointTrajectory::ROS_TYPE_NAME,
            "trajectory_msgs/msg/JointTrajectory"
        );
    }

    /// The name and the id name the same type: the registry finds a message by
    /// the identity its own struct carries, in either ROS spelling, and that is
    /// the type whose ids `AroraType` derives.
    #[test]
    fn the_registry_finds_a_message_by_its_own_identity() {
        let registry = crate::registry();
        let by_name = registry.get_by_name(Image::ROS_TYPE_NAME).unwrap();
        assert_eq!(by_name.id, Image::arora_type_id());
        assert_eq!(
            registry.id_of("sensor_msgs/Image"),
            Some(Image::arora_type_id())
        );
    }
}
