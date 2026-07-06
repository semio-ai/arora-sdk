use crate::config::{
    JointIdMapping, JointStateConversion, ROS2RobotConfig, TopicConfig, TopicDirection,
    TopicMapping,
};
use crate::msgs;

/// Create the configuration for a Quori robot
pub fn create_config() -> ROS2RobotConfig {
    let topics = vec![
        // Configuration for subscribing to joint state data.
        // Maps the "/joint_states" topic (sensor_msgs/JointState) to Arora joint state keys.
        TopicConfig::new::<msgs::JointState>(
            "/joint_states",
            TopicDirection::Subscribe,
            TopicMapping::JointState {
                conversion: JointStateConversion::Standard,
            },
        ),
        // Configuration for publishing joint commands using Float64MultiArray.
        // Maps Arora joint commands to the "/forward_position_controller/commands" topic (std_msgs/Float64MultiArray).
        // The mapping uses the JointState mapping with a Standard conversion, which implies the Arora joint state keys
        // are directly converted to a Float64MultiArray.
        TopicConfig::new::<msgs::Float64MultiArray>(
            "/forward_position_controller/commands",
            TopicDirection::Publish,
            TopicMapping::JointState {
                conversion: JointStateConversion::Standard,
            },
        ),
    ];

    let glb_path = default_model_path!("quori");

    ROS2RobotConfig {
        model_family: Some("quori".to_string()),
        domain_id: None,
        topics,
        joint_ids: JointIdMapping::FromGLB,
        model_glb_path: Some(glb_path.to_string()),
        ..Default::default()
    }
}
