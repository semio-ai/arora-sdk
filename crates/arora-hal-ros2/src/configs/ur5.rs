use std::collections::HashMap;

use crate::config::{
    JointIdMapping, JointStateConversion, ROS2RobotConfig, TopicConfig, TopicDirection,
    TopicMapping,
};
use crate::msgs;

/// Create the configuration for a UR5 robot
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

    let glb_path = default_model_path!("ur5");

    ROS2RobotConfig {
        model_family: Some("ur5".to_string()),
        domain_id: None,
        topics,
        joint_ids: JointIdMapping::FromGLB,
        model_glb_path: Some(glb_path.to_string()),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{JointStateConversion, TopicDirection, TopicMapping};

    #[test]
    fn test_ur5_config_creation() {
        let config = create_config();

        // Test domain ID
        assert_eq!(config.domain_id, None);

        // Test topics
        assert_eq!(config.topics.len(), 2);
    }

    #[test]
    fn test_ur5_joint_states_topic() {
        let config = create_config();

        let joint_states_topic = config
            .topics
            .iter()
            .find(|t| t.name == "/joint_states")
            .expect("Joint states topic should exist");

        assert_eq!(joint_states_topic.message_type, "sensor_msgs/JointState");
        assert!(matches!(
            joint_states_topic.direction,
            TopicDirection::Subscribe
        ));

        match &joint_states_topic.mapping {
            TopicMapping::JointState { conversion } => {
                assert!(matches!(conversion, JointStateConversion::Standard));
            }
            _ => panic!("Expected JointState mapping"),
        }
    }

    #[test]
    fn test_ur5_joint_trajectory_topic() {
        let config = create_config();

        let trajectory_topic = config
            .topics
            .iter()
            .find(|t| t.name == "/joint_trajectory")
            .expect("Joint trajectory topic should exist");

        assert_eq!(
            trajectory_topic.message_type,
            "trajectory_msgs/JointTrajectory"
        );
        assert!(matches!(
            trajectory_topic.direction,
            TopicDirection::Publish
        ));

        match &trajectory_topic.mapping {
            TopicMapping::StandardMessage { field_mappings } => {
                assert!(field_mappings.contains_key("points[0].positions"));
                assert_eq!(
                    field_mappings.get("points[0].positions"),
                    Some(&"positions".to_string())
                );
            }
            _ => panic!("Expected StandardMessage mapping"),
        }
    }

    #[test]
    fn test_ur5_config_validation() {
        let config = create_config();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_ur5_config_serialization() {
        let config = create_config();

        // Test that it can be serialized to JSON
        let json = serde_json::to_string(&config).expect("Should serialize to JSON");
        assert!(json.contains("/joint_states"));

        // Test that it can be deserialized from JSON
        let deserialized: ROS2RobotConfig =
            serde_json::from_str(&json).expect("Should deserialize from JSON");
        assert_eq!(deserialized.topics.len(), config.topics.len());
    }

    #[test]
    fn test_ur5_config_clone() {
        let config = create_config();
        let cloned = config.clone();

        assert_eq!(config.topics.len(), cloned.topics.len());
        assert_eq!(config.topics[0].name, cloned.topics[0].name);
    }
}
