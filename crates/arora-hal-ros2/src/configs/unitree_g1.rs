use std::collections::HashMap;

use crate::config::{
    JointIdMapping, JointStateConversion, ROS2RobotConfig, TopicConfig, TopicDirection,
    TopicMapping,
};
use crate::msgs;

/// Create the configuration for a Unitree G1 robot
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

    // Configuration for publishing joint trajectory commands.
    // Maps Arora joint commands to the "/joint_trajectory" topic (trajectory_msgs/JointTrajectory).
    // Specifically maps the 'positions' key from Arora to 'points[0].positions' in the ROS2 message.
    topics.push(TopicConfig::new::<msgs::JointTrajectory>(
        "/joint_trajectory",
        TopicDirection::Publish,
        TopicMapping::StandardMessage {
            field_mappings: {
                let mut mappings = HashMap::new();
                mappings.insert("points[0].positions".to_string(), "positions".to_string());
                mappings
            },
        },
    ));

    // Configuration for publishing tool speed commands.
    // Maps Arora tool speed commands to the "/ur3/tool_speed" topic (geometry_msgs/Twist).
    // Maps 'tool_speed.x', 'tool_speed.y', 'tool_speed.z' from Arora to 'linear.x', 'linear.y', 'linear.z' in the ROS2 message.
    topics.push(TopicConfig::new::<msgs::Twist>(
        "/ur3/tool_speed",
        TopicDirection::Publish,
        TopicMapping::StandardMessage {
            field_mappings: {
                let mut mappings = HashMap::new();
                mappings.insert("linear.x".to_string(), "tool_speed.x".to_string());
                mappings.insert("linear.y".to_string(), "tool_speed.y".to_string());
                mappings.insert("linear.z".to_string(), "tool_speed.z".to_string());
                mappings
            },
        },
    ));

    let glb_path = default_model_path!("g1");

    ROS2RobotConfig {
        model_family: Some("g1".to_string()),
        domain_id: None,
        topics,
        joint_ids: JointIdMapping::FromGLB,
        model_glb_path: Some(glb_path.to_string()),
        ..Default::default()
    }
}
