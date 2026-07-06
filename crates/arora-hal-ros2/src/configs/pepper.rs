use std::collections::HashMap;

use crate::config::{
    JointIdMapping, JointStateConversion, ROS2RobotConfig, TopicConfig, TopicDirection,
    TopicMapping,
};
use crate::msgs;

/// Create the configuration for a Pepper robot
pub fn create_config() -> ROS2RobotConfig {
    let mut topics = Vec::new();

    // Configuration for subscribing to joint state data.
    // Maps the "/joint_states" topic (sensor_msgs/JointState) to Arora joint state keys.
    topics.push(TopicConfig::new::<msgs::JointState>(
        "/joint_states",
        TopicDirection::Subscribe,
        TopicMapping::JointState {
            conversion: JointStateConversion::Standard,
        },
    ));

    // Configuration for publishing joint angle commands.
    // Maps Arora joint commands to the "/joint_angles" topic (naoqi_bridge_msgs/JointAnglesWithSpeed).
    // Uses the JointAngles mapping for NAO/Pepper robot joint control.
    topics.push(TopicConfig::new::<msgs::JointAnglesWithSpeed>(
        "/joint_angles",
        TopicDirection::Publish,
        TopicMapping::JointAngles,
    ));

    // Configuration for publishing joint trajectory commands.
    // Maps Arora joint commands to the "/joint_trajectory" topic (trajectory_msgs/JointTrajectory).
    topics.push(TopicConfig::new::<msgs::JointTrajectory>(
        "/joint_trajectory",
        TopicDirection::Publish,
        TopicMapping::JointTrajectory,
    ));

    // Configuration for publishing speech commands.
    // Maps Arora text commands to the "/speech" topic (std_msgs/String).
    // Maps the 'text' key from Arora to the 'data' field in the ROS2 message.
    topics.push(TopicConfig::new::<msgs::String>(
        "/speech",
        TopicDirection::Publish,
        TopicMapping::StandardMessage {
            field_mappings: {
                let mut mappings = HashMap::new();
                mappings.insert("data".to_string(), "text".to_string());
                mappings
            },
        },
    ));

    // Configuration for publishing tablet image data.
    // Maps Arora image data to the "/tablet/image" topic (sensor_msgs/Image).
    // Maps the 'image_data' key from Arora to the 'data' field in the ROS2 message.
    // NOTE: Image type is not yet defined in msgs module, so we use the plain struct initialization
    topics.push(TopicConfig {
        name: "/tablet/image".to_string(),
        message_type: "sensor_msgs/Image".to_string(),
        direction: TopicDirection::Publish,
        mapping: TopicMapping::StandardMessage {
            field_mappings: {
                let mut mappings = HashMap::new();
                mappings.insert("data".to_string(), "image_data".to_string());
                mappings
            },
        },
    });

    // Configuration for publishing base velocity commands.
    // Maps Arora velocity commands to the "/cmd_vel" topic (geometry_msgs/Twist).
    // Maps 'velocity.x' and 'rotation.z' from Arora to 'linear.x' and 'angular.z' in the ROS2 message.
    topics.push(TopicConfig::new::<msgs::Twist>(
        "/cmd_vel",
        TopicDirection::Publish,
        TopicMapping::StandardMessage {
            field_mappings: {
                let mut mappings = HashMap::new();
                mappings.insert("linear.x".to_string(), "velocity.x".to_string());
                mappings.insert("angular.z".to_string(), "rotation.z".to_string());
                mappings
            },
        },
    ));

    let glb_path = default_model_path!("pepper");

    ROS2RobotConfig {
        model_family: Some("pepper".to_string()),
        domain_id: None,
        topics,
        joint_ids: JointIdMapping::FromGLB,
        model_glb_path: Some(glb_path.to_string()),
        ..Default::default()
    }
}
