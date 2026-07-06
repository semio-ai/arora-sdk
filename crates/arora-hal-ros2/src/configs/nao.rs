use std::collections::HashMap;

use crate::config::{
    JointIdMapping, JointStateConversion, ROS2RobotConfig, TopicConfig, TopicDirection,
    TopicMapping,
};
use crate::msgs;

/// Create the configuration for a NAO robot
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
    topics.push(TopicConfig::new::<msgs::JointTrajectory>(
        "/joint_trajectory",
        TopicDirection::Publish,
        TopicMapping::JointTrajectory,
    ));

    topics.push(TopicConfig::new::<msgs::JointAnglesWithSpeed>(
        "/joint_angles",
        TopicDirection::Publish,
        TopicMapping::JointAngles,
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

    let glb_path = default_model_path!("nao");

    ROS2RobotConfig {
        model_family: Some("nao".to_string()),
        domain_id: None,
        topics,
        joint_ids: JointIdMapping::FromGLB,
        model_glb_path: Some(glb_path.to_string()),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::time::Duration;

    use rand::Rng;
    use ros2_client::{NodeName as RosNodeName, NodeOptions, DEFAULT_PUBLISHER_QOS};

    use arora_hal::Hal;
    use arora_types::data::{Key, StateChange};
    use arora_types::value::Value;

    use crate::config::{
        JointIdMapping, ROS2RobotConfig, TopicConfig, TopicDirection, TopicMapping,
    };
    use crate::msgs::{self, MessageType};
    use crate::ros2_hal::Ros2Hal;

    /// Test that writing state changes with joint IDs via the HAL
    /// results in a JointAnglesWithSpeed message on `/joint_angles` with:
    /// - Joint ROS names (not IDs)
    /// - No duplicate entries
    #[tokio::test]
    #[cfg_attr(
        target_os = "macos",
        ignore = "DDS multicast SPDP discovery is unreliable on macOS loopback (rustdds 0.11 \
                  has no unicast-peer/interface config); these run on Linux CI. To run locally, \
                  ensure an active multicast-capable interface and use `--ignored`."
    )]
    async fn test_nao_publish_joint_angles_no_duplicates() {
        let domain_id: u16 = rand::rng().random_range(1..=200);

        // Build a NAO-like config with only the /joint_angles topic,
        // using Override mapping so we don't need the GLB file.
        let config = ROS2RobotConfig {
            domain_id: Some(domain_id),
            topics: vec![TopicConfig::new::<msgs::JointAnglesWithSpeed>(
                "/joint_angles",
                TopicDirection::Publish,
                TopicMapping::JointAngles,
            )],
            joint_ids: JointIdMapping::Override(HashMap::from([
                ("HeadYaw".to_string(), "head_yaw".to_string()),
                ("HeadPitch".to_string(), "head_pitch".to_string()),
                ("LShoulderPitch".to_string(), "l_shoulder_pitch".to_string()),
            ])),
            ..Default::default()
        };

        let hal = Arc::new(Ros2Hal::new(config).await.expect("failed to create HAL"));

        // Create a separate subscriber node to receive messages from the HAL
        let context_options = ros2_client::ContextOptions::new().domain_id(domain_id);
        let sub_ctx = ros2_client::Context::with_options(context_options)
            .expect("failed to create subscriber context");

        let sub_node_name =
            RosNodeName::new("/", &format!("test_nao_joint_angles_sub_{domain_id}"))
                .expect("valid node name");
        let mut sub_node = sub_ctx
            .new_node(sub_node_name, NodeOptions::new())
            .expect("failed to create subscriber node");
        tokio::spawn(sub_node.spinner().unwrap().spin());

        let topic_name = ros2_client::Name::parse("/joint_angles").expect("valid topic name");
        let sub_topic = sub_node
            .create_topic(
                &topic_name,
                msgs::JointAnglesWithSpeed::message_type_name(),
                &DEFAULT_PUBLISHER_QOS,
            )
            .expect("create subscriber topic");
        let subscriber = sub_node
            .create_subscription::<msgs::JointAnglesWithSpeed>(&sub_topic, None)
            .expect("create subscriber");

        // Give time for DDS discovery
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Create a state change using joint IDs (not ROS names),
        // with both target_position and current_position components,
        // as would happen when commands combine with sensor feedback.
        let mut state_change = StateChange::new();
        state_change.set.insert(
            Key::from("head_yaw.target_position".to_string()),
            Some(Value::from(0.5)),
        );
        state_change.set.insert(
            Key::from("head_yaw.current_position".to_string()),
            Some(Value::from(0.48)),
        );
        state_change.set.insert(
            Key::from("head_pitch.target_position".to_string()),
            Some(Value::from(-0.3)),
        );
        state_change.set.insert(
            Key::from("head_pitch.current_position".to_string()),
            Some(Value::from(-0.28)),
        );
        state_change.set.insert(
            Key::from("l_shoulder_pitch.target_position".to_string()),
            Some(Value::from(1.0)),
        );
        state_change.set.insert(
            Key::from("l_shoulder_pitch.current_position".to_string()),
            Some(Value::from(0.95)),
        );

        // Spawn a task to repeatedly publish via the HAL's write method
        let state_change_clone = state_change.clone();
        let writer_hal = hal.clone();
        tokio::spawn(async move {
            loop {
                let _ = writer_hal.write(state_change_clone.clone()).await;
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        });

        // Wait for the message to be received
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        let msg = loop {
            if tokio::time::Instant::now() >= deadline {
                panic!("timeout waiting for JointAnglesWithSpeed message");
            }
            match subscriber.async_take().await {
                Ok((msg, _info)) => break msg,
                Err(_) => {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            }
        };

        println!("Received JointAnglesWithSpeed message: {:?}", msg);

        // 1. Verify joint ROS names are used (not IDs)
        assert!(
            msg.joint_names.contains(&"HeadYaw".to_string()),
            "Message should contain ROS name 'HeadYaw', got: {:?}",
            msg.joint_names
        );
        assert!(
            msg.joint_names.contains(&"HeadPitch".to_string()),
            "Message should contain ROS name 'HeadPitch', got: {:?}",
            msg.joint_names
        );
        assert!(
            msg.joint_names.contains(&"LShoulderPitch".to_string()),
            "Message should contain ROS name 'LShoulderPitch', got: {:?}",
            msg.joint_names
        );

        // 2. Verify no duplicate joint names
        let mut seen = std::collections::HashSet::new();
        for name in &msg.joint_names {
            assert!(
                seen.insert(name.clone()),
                "Duplicate joint name found: '{}'. All names: {:?}",
                name,
                msg.joint_names
            );
        }

        // 3. Verify the angles match the positions we sent
        assert_eq!(
            msg.joint_names.len(),
            msg.joint_angles.len(),
            "joint_names and joint_angles should have the same length"
        );
        assert_eq!(
            msg.joint_names.len(),
            3,
            "Should have exactly 3 joints, got {}",
            msg.joint_names.len()
        );

        let head_yaw_idx = msg
            .joint_names
            .iter()
            .position(|n| n == "HeadYaw")
            .expect("HeadYaw index");
        let head_pitch_idx = msg
            .joint_names
            .iter()
            .position(|n| n == "HeadPitch")
            .expect("HeadPitch index");
        let l_shoulder_idx = msg
            .joint_names
            .iter()
            .position(|n| n == "LShoulderPitch")
            .expect("LShoulderPitch index");

        assert!(
            (msg.joint_angles[head_yaw_idx] - 0.5).abs() < 0.001,
            "HeadYaw angle should be 0.5, got: {}",
            msg.joint_angles[head_yaw_idx]
        );
        assert!(
            (msg.joint_angles[head_pitch_idx] - (-0.3)).abs() < 0.001,
            "HeadPitch angle should be -0.3, got: {}",
            msg.joint_angles[head_pitch_idx]
        );
        assert!(
            (msg.joint_angles[l_shoulder_idx] - 1.0).abs() < 0.001,
            "LShoulderPitch angle should be 1.0, got: {}",
            msg.joint_angles[l_shoulder_idx]
        );

        println!(
            "Successfully verified JointAnglesWithSpeed message with {} joints",
            msg.joint_names.len()
        );
    }
}
