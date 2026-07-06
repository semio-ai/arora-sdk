mod converting_publisher;
pub(crate) use converting_publisher::*;

mod from_state_change;
pub use from_state_change::*;

mod to_state_change;
pub use to_state_change::*;

use std::{any::TypeId, collections::HashMap};

use arora_types::data::{Key, StateChange};
use log::{debug, error};

use crate::config::TopicMapping;
use crate::msgs::{Float64MultiArray, JointAnglesWithSpeed, JointState, JointTrajectory};
use crate::ros2_error::ROS2RobotError;

/// Converts a ROS2 message into an Arora `StateChange` based on the provided topic mapping.
///
/// # Arguments
///
/// * `message` - The ROS2 message.
/// * `mapping` - The topic mapping configuration.
/// * `topic_name` - The name of the topic the message was received on.
/// * `joint_ids` - The mapping from ROS joint names to Arora joint IDs.
///
/// # Returns
///
/// A `Result` containing the `StateChange` or a `ROS2RobotError` if conversion fails.
pub fn message_to_state_change<T: ToStateChange + 'static>(
    message: T,
    mapping: &TopicMapping,
    topic_name: &str,
    joint_ids: &HashMap<String, String>,
) -> Result<StateChange, ROS2RobotError> {
    debug!(
        "Attempting to convert ROS2 message from topic '{}' with mapping {:?}",
        topic_name, mapping
    );
    let type_id = TypeId::of::<T>();
    match mapping {
        TopicMapping::JointState { conversion: _ } => {
            debug!("Mapping is JointState for topic '{}'", topic_name);
            check_type_match::<JointState>(type_id, topic_name)?;
        }
        TopicMapping::JointTrajectory => {
            debug!("Mapping is JointTrajectory for topic '{}'", topic_name);
            check_type_match::<JointTrajectory>(type_id, topic_name)?;
        }
        TopicMapping::JointAngles => {
            debug!("Mapping is JointAngles for topic '{}'", topic_name);
            check_type_match::<JointAnglesWithSpeed>(type_id, topic_name)?;
        }
        TopicMapping::Float64MultiArray => {
            debug!("Mapping is Float64MultiArray for topic '{}'", topic_name);
            check_type_match::<Float64MultiArray>(type_id, topic_name)?;
        }
        TopicMapping::StandardMessage { field_mappings } => {
            debug!("Mapping is StandardMessage for topic '{}'", topic_name);
            // StandardMessage can be used with various types including msgs::String
            // Convert first, then apply field mappings
            let mut state_change = message.into_state_change(joint_ids)?;

            // Apply field mappings: rename keys from ROS field names to Arora key names
            for (ros_field, arora_field) in field_mappings {
                let ros_key = Key::from(ros_field.clone());
                if let Some(value) = state_change.set.remove(&ros_key) {
                    let arora_key = Key::from(arora_field.clone());
                    state_change.set.insert(arora_key, value);
                    debug!("Renamed field '{}' to '{}'", ros_field, arora_field);
                }
            }

            return Ok(state_change);
        }
    };
    message.into_state_change(joint_ids)
}

/// Checks if the actual type of a ROS2 message matches the expected type based on the topic mapping.
/// Produces an error with a regular message if the types do not match.
fn check_type_match<ExpectedType: 'static>(
    actual_type: TypeId,
    topic_name: &str,
) -> Result<(), ROS2RobotError> {
    let expected_type = TypeId::of::<ExpectedType>();
    if actual_type != expected_type {
        let error_message = format!(
            "Type mismatch for topic '{}': expected {:?}, but got {:?}",
            topic_name, expected_type, actual_type
        );
        error!("{}", error_message);
        Err(ROS2RobotError::ConversionError(error_message))
    } else {
        Ok(())
    }
}

/// Converts an Arora `StateChange` into a ROS2 message based on the provided topic mapping.
///
/// # Arguments
///
/// * `state_change` - The Arora `StateChange` to convert.
/// * `mapping` - The topic mapping configuration.
/// * `topic_name` - The name of the target ROS2 topic.
/// * `joint_ids_to_ros_names` - The mapping from Arora joint IDs to ROS joint names.
///
/// # Returns
///
/// A `Result` containing the ROS2 message of type `T` or a `ROS2RobotError` if conversion fails.
pub fn state_change_to_message<T: FromStateChange + 'static>(
    state_change: &StateChange,
    mapping: &TopicMapping,
    topic_name: &str,
    joint_ids_to_ros_names: &HashMap<String, String>,
) -> Result<Option<T>, ROS2RobotError> {
    debug!(
        "Attempting to convert StateChange with {} keys to ROS2 message for topic '{}' with mapping {:?}",
        state_change.set.len(),
        topic_name,
        mapping
    );
    let type_id = TypeId::of::<T>();
    match mapping {
        TopicMapping::JointState { conversion: _ } => {
            debug!("Mapping is JointState for topic '{}'", topic_name);
            if check_type_match::<JointState>(type_id, topic_name).is_ok()
                || check_type_match::<Float64MultiArray>(type_id, topic_name).is_ok()
            {
                debug!("Target type is JointState for topic '{}'", topic_name);
            } else {
                let error_message = format!(
                    "Type mismatch for topic '{}': expected JointState or Float64MultiArray, but target type is {:?}",
                    topic_name, type_id
                );
                error!("{}", error_message);
                return Err(ROS2RobotError::ConversionError(error_message));
            }
        }
        TopicMapping::JointAngles => {
            debug!("Mapping is JointAngles for topic '{}'", topic_name);
            check_type_match::<JointAnglesWithSpeed>(type_id, topic_name)?;
        }
        TopicMapping::JointTrajectory => {
            debug!("Mapping is JointTrajectory for topic '{}'", topic_name);
            check_type_match::<JointTrajectory>(type_id, topic_name)?;
        }
        TopicMapping::Float64MultiArray => {
            debug!("Mapping is Float64MultiArray for topic '{}'", topic_name);
            check_type_match::<Float64MultiArray>(type_id, topic_name)?;
        }
        TopicMapping::StandardMessage { field_mappings } => {
            debug!("Mapping is StandardMessage for topic '{}'", topic_name);
            // StandardMessage can be used with various types including msgs::String
            // Apply reverse field mappings: rename keys from Arora key names to ROS field names
            let mut mapped_state_change = state_change.clone();
            for (ros_field, arora_field) in field_mappings {
                let arora_key = Key::from(arora_field.clone());
                if let Some(value) = mapped_state_change.set.remove(&arora_key) {
                    let ros_key = Key::from(ros_field.clone());
                    mapped_state_change.set.insert(ros_key, value);
                    debug!(
                        "Renamed field '{}' to '{}' for publishing",
                        arora_field, ros_field
                    );
                }
            }
            return T::from_state_change(&mapped_state_change, joint_ids_to_ros_names);
        }
    };
    T::from_state_change(state_change, joint_ids_to_ros_names)
}

#[cfg(test)]
mod tests {
    use arora_types::value::Value;

    use crate::get_now;
    use crate::msgs;

    use super::*;

    fn create_test_joint_state() -> JointState {
        JointState {
            header: msgs::Header {
                stamp: get_now(),
                frame_id: "base_link".to_string(),
            },
            name: vec![
                "joint1".to_string(),
                "joint2".to_string(),
                "joint3".to_string(),
            ],
            position: vec![1.0, 2.0, 3.0],
            velocity: vec![0.1, 0.2, 0.3],
            effort: vec![10.0, 20.0, 30.0],
        }
    }

    fn create_test_state_change() -> StateChange {
        let mut state_change = StateChange::new();

        // Add some joint position targets
        state_change.set.insert(
            Key::from("joint1.target_position".to_string()),
            Some(Value::from(1.5_f64)),
        );
        state_change.set.insert(
            Key::from("joint2.target_position".to_string()),
            Some(Value::from(2.5_f64)),
        );
        state_change.set.insert(
            Key::from("joint3.target_position".to_string()),
            Some(Value::from(3.5_f64)),
        );

        state_change
    }

    #[test]
    fn test_joint_state_to_state_change_success() {
        let joint_state = create_test_joint_state();
        let joint_ids = HashMap::new();

        let result = joint_state.into_state_change(&joint_ids);

        assert!(result.is_ok());
        let state_change = result.unwrap();

        // Should have position, velocity, and effort keys for each joint
        assert_eq!(state_change.set.len(), 9); // 3 joints × 3 properties

        // Check that we have the expected keys
        assert!(state_change
            .set
            .contains_key(&Key::from("joint1.position".to_string())));
        assert!(state_change
            .set
            .contains_key(&Key::from("joint1.velocity".to_string())));
        assert!(state_change
            .set
            .contains_key(&Key::from("joint1.effort".to_string())));
    }

    #[test]
    fn test_joint_state_to_state_change_mismatched_arrays() {
        let mut joint_state = create_test_joint_state();
        joint_state.position = vec![1.0, 2.0]; // Fewer positions than names
        let joint_ids = HashMap::new();

        let result = joint_state.into_state_change(&joint_ids);

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("different lengths"));
    }

    #[test]
    fn test_joint_state_to_state_change_empty_arrays() {
        let mut joint_state = create_test_joint_state();
        joint_state.name.clear();
        joint_state.position.clear();
        joint_state.velocity.clear();
        joint_state.effort.clear();
        let joint_ids = HashMap::new();

        let result = joint_state.into_state_change(&joint_ids);

        assert!(result.is_ok());
        let state_change = result.unwrap();
        assert_eq!(state_change.set.len(), 0);
    }

    #[test]
    fn test_state_change_to_joint_state_success() {
        let state_change = create_test_state_change();
        let joint_ids = HashMap::new();

        let result = JointState::from_state_change(&state_change, &joint_ids);

        assert!(result.is_ok());
        let joint_state = result.unwrap().unwrap();

        // Should have 3 joints
        assert_eq!(joint_state.name.len(), 3);
        assert_eq!(joint_state.position.len(), 3);

        // Check that joint names and positions are correctly extracted
        // Note: HashMap iteration order is not guaranteed, so we check for presence
        assert!(joint_state.name.contains(&"joint1".to_string()));
        assert!(joint_state.name.contains(&"joint2".to_string()));
        assert!(joint_state.name.contains(&"joint3".to_string()));

        // Find the position for joint1
        if let Some(index) = joint_state.name.iter().position(|name| name == "joint1") {
            assert_eq!(joint_state.position[index], 1.5);
        }
    }

    #[test]
    fn test_state_change_to_joint_state_no_positions() {
        let state_change = StateChange::new(); // Empty state change
        let joint_ids = HashMap::new();
        let result = JointState::from_state_change(&state_change, &joint_ids);

        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_roundtrip_conversion() {
        let original_joint_state = create_test_joint_state();
        let joint_ids = HashMap::new();

        // Convert JointState -> StateChange
        original_joint_state.into_state_change(&joint_ids).unwrap();

        // Create a new StateChange with target positions for conversion back
        let mut target_state_change = StateChange::new();
        target_state_change.set.insert(
            Key::from("joint1.target_position".to_string()),
            Some(Value::from(1.0_f64)),
        );
        target_state_change.set.insert(
            Key::from("joint2.target_position".to_string()),
            Some(Value::from(2.0_f64)),
        );
        target_state_change.set.insert(
            Key::from("joint3.target_position".to_string()),
            Some(Value::from(3.0_f64)),
        );

        // Convert StateChange -> JointState
        let converted_joint_state = JointState::from_state_change(&target_state_change, &joint_ids)
            .unwrap()
            .unwrap();

        // Should have same number of joints
        assert_eq!(converted_joint_state.name.len(), 3);
        assert_eq!(converted_joint_state.position.len(), 3);
    }

    fn create_test_joint_angles() -> JointAnglesWithSpeed {
        JointAnglesWithSpeed {
            header: msgs::Header {
                stamp: get_now(),
                frame_id: "".to_string(),
            },
            joint_names: vec![
                "HeadYaw".to_string(),
                "HeadPitch".to_string(),
                "LShoulderPitch".to_string(),
            ],
            joint_angles: vec![0.5, 0.1, -0.3],
            speed: 0.2,
            relative: 0,
        }
    }

    #[test]
    fn test_joint_angles_to_state_change_success() {
        let joint_angles = create_test_joint_angles();
        let joint_ids = HashMap::new();

        let result = joint_angles.into_state_change(&joint_ids);

        assert!(result.is_ok());
        let state_change = result.unwrap();
        assert_eq!(state_change.set.len(), 3);
        println!(
            "StateChange keys: {:?}",
            state_change.set.keys().collect::<Vec<_>>()
        );

        // Check that we have the expected joint positions
        let head_yaw_key = Key::from("HeadYaw.position".to_string());
        assert!(state_change.set.contains_key(&head_yaw_key));
        if let Some(Some(Value::F64(position))) = state_change.set.get(&head_yaw_key) {
            assert!((position - 0.5).abs() < f64::EPSILON);
        } else {
            panic!("Should contain HeadYaw position as F64");
        }
    }

    #[test]
    fn test_joint_angles_to_state_change_mismatched_arrays() {
        let mut joint_angles = create_test_joint_angles();
        joint_angles.joint_angles = vec![0.5, 0.1]; // Fewer angles than names
        let joint_ids = HashMap::new();

        let result = joint_angles.into_state_change(&joint_ids);

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("different lengths"));
    }

    #[test]
    fn test_state_change_to_joint_angles_success() {
        let mut state_change = StateChange::new();
        state_change.set.insert(
            Key::from("HeadYaw.target_position".to_string()),
            Some(Value::F64(0.5)),
        );
        state_change.set.insert(
            Key::from("HeadPitch.target_position".to_string()),
            Some(Value::F64(0.1)),
        );
        let joint_ids = HashMap::new();

        let result = JointAnglesWithSpeed::from_state_change(&state_change, &joint_ids);

        assert!(result.is_ok());
        let joint_angles = result.unwrap().unwrap();
        assert_eq!(joint_angles.joint_names.len(), 2);
        assert_eq!(joint_angles.joint_angles.len(), 2);
        println!(
            "JointAngles names: {:?}",
            joint_angles.joint_names.iter().collect::<Vec<_>>()
        );
        assert!(joint_angles.joint_names.contains(&"HeadYaw".to_string()));
        assert!(joint_angles.joint_names.contains(&"HeadPitch".to_string()));
    }
}
