//! ROS 2 robots as Arora HALs.
//!
//! One [`arora_hal::Hal`] implementation, [`Ros2Hal`], drives any ROS 2 robot
//! from a [`ROS2RobotConfig`]: topics map to Arora keys/values, and the
//! configuration decides which robot it is. Ready-made configurations for
//! well-known robots (NAO, Pepper, Quori, UR3/UR5, Unitree G1) live in
//! [`configs`]; a JSON file deserializes to the same [`ROS2RobotConfig`].

mod config;
pub use config::{
    JointStateConversion, ROS2RobotConfig, TopicConfig, TopicDirection, TopicMapping,
};

pub mod configs;
pub use configs::*;

mod conversions;

mod ros2_error;
pub use ros2_error::ROS2RobotError;

mod ros2_hal;
pub use ros2_hal::Ros2Hal;

mod ros2_msgs;
pub mod msgs {
    pub use super::ros2_msgs::*;
}

pub fn get_now() -> ros2_client::builtin_interfaces::Time {
    let nanos = chrono::Utc::now().timestamp_nanos_opt().unwrap(); // Dead old devices are not supported
    ros2_client::builtin_interfaces::Time::from_nanos(nanos)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use arora_types::data::{Key, StateChange};
    use arora_types::value::Value;

    use super::*;
    use crate::config::get_joint_ids_from_glb_file;
    use crate::conversions::{FromStateChange, ToStateChange};

    fn create_test_config() -> ROS2RobotConfig {
        ROS2RobotConfig {
            domain_id: Some(42),
            ..Default::default()
        }
    }

    #[test]
    fn test_module_exports() {
        // Test that important types are exported
        let _config: ROS2RobotConfig = create_test_config();
        let _error: ROS2RobotError = ROS2RobotError::ConfigError("test".to_string());

        // Test that config enums are exported
        let _direction = TopicDirection::Subscribe;
        let _mapping = TopicMapping::JointAngles;
        let _conversion = JointStateConversion::Standard;
    }

    #[test]
    fn test_pre_built_configs_available() {
        // Test that pre-built configs are accessible
        let _ur5_config = configs::ur5::create_config();
        let _ur3_config = configs::ur3::create_config();
        let _nao_config = configs::nao::create_config();
        let _pepper_config = configs::pepper::create_config();
        let _quori_config = configs::quori::create_config();
        let _g1_config = configs::unitree_g1::create_config();
    }

    /// Round-trips a JointState conversion through the NAO configuration: the
    /// GLB joint-id mapping turns ROS joint names into Arora keys, and a
    /// target-position write converts back to a JointState naming the ROS
    /// joint.
    #[test]
    fn test_nao_config_joint_state_round_trip() {
        let config = configs::nao::create_config();
        let glb_path = config
            .model_glb_path
            .expect("NAO config should have a model path");
        let ros_names_to_ids =
            get_joint_ids_from_glb_file(&glb_path).expect("NAO GLB should parse");
        assert!(!ros_names_to_ids.is_empty(), "NAO GLB should define joints");
        let (ros_name, joint_id) = ros_names_to_ids.iter().next().unwrap();

        // ROS -> Arora: a JointState reading becomes "<joint_id>.position".
        let joint_state = msgs::JointState {
            header: msgs::Header {
                stamp: get_now(),
                frame_id: "base_link".to_string(),
            },
            name: vec![ros_name.clone()],
            position: vec![0.42],
            velocity: vec![],
            effort: vec![],
        };
        let change = joint_state
            .into_state_change(&ros_names_to_ids)
            .expect("conversion should succeed");
        let key = Key::from(format!("{joint_id}.position"));
        assert_eq!(change.set.get(&key), Some(&Some(Value::F64(0.42))));

        // Arora -> ROS: "<joint_id>.target_position" becomes a JointState
        // naming the ROS joint.
        let ids_to_ros_names: HashMap<String, String> = ros_names_to_ids
            .iter()
            .map(|(ros, id)| (id.clone(), ros.clone()))
            .collect();
        let mut target = StateChange::new();
        target.set.insert(
            Key::from(format!("{joint_id}.target_position")),
            Some(Value::F64(0.42)),
        );
        let message = msgs::JointState::from_state_change(&target, &ids_to_ros_names)
            .expect("conversion should succeed")
            .expect("a target position should produce a message");
        assert_eq!(message.name, vec![ros_name.clone()]);
        assert_eq!(message.position, vec![0.42]);
    }
}
