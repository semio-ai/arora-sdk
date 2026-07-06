use std::collections::HashMap;

use arora_types::data::{Key, StateChange};
use arora_types::{keyvalue::KeyValue, value::Value};
use log::debug;

use crate::{
    get_now,
    msgs::{
        self, Duration, Float64MultiArray, Header, JointAnglesWithSpeed, JointState,
        JointTrajectory, JointTrajectoryPoint, MultiArrayDimension, MultiArrayLayout,
    },
    ROS2RobotError,
};

pub trait FromStateChange: Sized {
    fn from_state_change(
        state_change: &StateChange,
        joint_ids_to_ros_names: &HashMap<String, String>,
    ) -> Result<Option<Self>, ROS2RobotError>;
}

// FromStateChange implementations
impl FromStateChange for JointState {
    fn from_state_change(
        state_change: &StateChange,
        joint_ids_to_ros_names: &HashMap<String, String>,
    ) -> Result<Option<Self>, ROS2RobotError> {
        debug!("Converting StateChange to JointState message");

        let mut names = Vec::new();
        let mut positions = Vec::new();

        // Extract joint positions from StateChange set values
        for (key, value_opt) in &state_change.set {
            // Look only for target positions component for this device (root namespace)
            if !key.get_namespace().is_empty() || key.get_component() != Some("target_position") {
                continue;
            }

            if let Some(value) = value_opt {
                let entity = key.get_entity().to_string();
                let joint_name = joint_ids_to_ros_names.get(&entity).unwrap_or(&entity);

                // Convert Value to f64
                let position = match value {
                    Value::F64(f) => *f,
                    Value::F32(f) => *f as f64,
                    Value::I64(i) => *i as f64,
                    Value::I32(i) => *i as f64,
                    _ => {
                        return Err(ROS2RobotError::ConversionError(format!(
                            "Unsupported value type for joint position: {value:?}"
                        )));
                    }
                };
                names.push(joint_name.to_string());
                positions.push(position);
            }
        }

        if names.is_empty() {
            return Ok(None);
        }

        assert!(names.len() == positions.len());
        let arrays_len = names.len();
        debug!("Successfully converted StateChange to JointState message with {arrays_len} joints",);
        Ok(Some(JointState {
            header: Header {
                stamp: get_now(),
                frame_id: "base_link".to_string(),
            },
            name: names,
            position: positions,
            velocity: vec![0.0; arrays_len],
            effort: vec![0.0; arrays_len],
        }))
    }
}

impl FromStateChange for Float64MultiArray {
    fn from_state_change(
        state_change: &StateChange,
        joint_ids_to_ros_names: &HashMap<String, String>,
    ) -> Result<Option<Self>, ROS2RobotError> {
        debug!("Converting StateChange to Float64MultiArray");
        let mut joint_positions: Vec<(String, f64)> = Vec::new();

        // Extract joint positions from state change
        for (key, value) in &state_change.set {
            // Look only for target positions component for this device (root namespace)
            if !key.get_namespace().is_empty() || key.get_component() != Some("target_position") {
                continue;
            }

            let entity = key.get_entity().to_string();
            let joint_name = joint_ids_to_ros_names.get(&entity).unwrap_or(&entity);
            if let Some(Value::F64(position)) = value {
                joint_positions.push((joint_name.to_string(), *position));
            }
        }

        if joint_positions.is_empty() {
            return Ok(None);
        }

        // Sort by joint name for consistent ordering
        joint_positions.sort_by(|a, b| a.0.cmp(&b.0));

        let names: Vec<String> = joint_positions
            .iter()
            .map(|(name, _)| name.clone())
            .collect();
        let positions: Vec<f64> = joint_positions.iter().map(|(_, pos)| *pos).collect();

        // Create dimensions with appropriate strides
        let mut dimensions = Vec::with_capacity(names.len());

        for (i, name) in names.iter().enumerate() {
            let stride = names.len() - i;
            dimensions.push(MultiArrayDimension {
                label: name.clone(),
                size: 1,
                stride: stride as u32,
            });
        }

        debug!("Successfully transformed StateChange to Float64MultiArray");
        Ok(Some(Float64MultiArray {
            layout: MultiArrayLayout {
                dim: dimensions,
                data_offset: 0,
            },
            data: positions,
        }))
    }
}

impl FromStateChange for JointAnglesWithSpeed {
    fn from_state_change(
        state_change: &StateChange,
        joint_ids_to_ros_names: &HashMap<String, String>,
    ) -> Result<Option<Self>, ROS2RobotError> {
        let joint_ids_string = format!("{:?}", joint_ids_to_ros_names);
        debug!(
            "Converting StateChange to JointAnglesWithSpeed, with joint IDs mapping: {joint_ids_string}"
        );

        let mut joint_positions = Vec::new();
        let mut joint_names = Vec::new();

        // Extract joint positions from key/value pairs
        for (key, value) in &state_change.set {
            // Only consider target_position components at root namespace
            if !key.get_namespace().is_empty() || key.get_component() != Some("target_position") {
                continue;
            }

            let key_str = &key.path;
            let conversion_result = match value {
                Some(Value::F64(value)) => Ok(value),
                Some(_) => Err("value is not a f64"),
                None => Err("no value associated"),
            };

            match conversion_result {
                Ok(value) => {
                    let entity = key.get_entity().to_string();
                    let joint_name = joint_ids_to_ros_names.get(&entity).unwrap_or(&entity);

                    joint_names.push(joint_name.to_string());
                    joint_positions.push(*value as f32);
                }
                Err(msg) => {
                    debug!("Key {key_str} ignored in conversion to JointAnglesWithSpeed: {msg}")
                }
            }
        }

        if joint_names.is_empty() {
            return Ok(None);
        }

        debug!("Successfully converted StateChange to JointAnglesWithSpeed");
        Ok(Some(JointAnglesWithSpeed {
            header: Header {
                stamp: get_now(),
                frame_id: "".to_string(),
            },
            joint_names,
            joint_angles: joint_positions,
            speed: 0.3,  // Default to 30% of max velocity
            relative: 0, // Default to absolute positioning
        }))
    }
}

impl FromStateChange for JointTrajectory {
    fn from_state_change(
        state_change: &StateChange,
        _: &HashMap<String, String>,
    ) -> Result<Option<Self>, ROS2RobotError> {
        debug!("Converting StateChange to JointTrajectory");

        // Look for trajectory data in the StateChange
        let trajectory_key = Key::from("common.target_trajectory".to_string());

        let key_value = match state_change.set.get(&trajectory_key) {
            Some(Some(Value::KeyValue(kv))) => kv,
            Some(None) | None => return Ok(None),
            _ => {
                return Err(ROS2RobotError::ConversionError(
                    "Expected KeyValue for trajectory data".to_string(),
                ));
            }
        };

        // Extract joint names
        let joint_names_field = key_value.fields.get("jointNames").ok_or_else(|| {
            ROS2RobotError::ConversionError("Missing 'jointNames' field in trajectory".to_string())
        })?;

        let joint_names = match &joint_names_field.value {
            Some(boxed_value) => match boxed_value.as_ref() {
                Value::ArrayString(names) => names.clone(),
                _ => {
                    return Err(ROS2RobotError::ConversionError(
                        "Expected ArrayString for jointNames".to_string(),
                    ));
                }
            },
            None => {
                return Err(ROS2RobotError::ConversionError(
                    "jointNames field has no value".to_string(),
                ));
            }
        };

        // Extract points
        let points_field = key_value.fields.get("points").ok_or_else(|| {
            ROS2RobotError::ConversionError("Missing 'points' field in trajectory".to_string())
        })?;

        let points_array = match &points_field.value {
            Some(boxed_value) => match boxed_value.as_ref() {
                Value::ArrayValue(points) => points,
                _ => {
                    return Err(ROS2RobotError::ConversionError(
                        "Expected ArrayValue for points".to_string(),
                    ));
                }
            },
            None => {
                return Err(ROS2RobotError::ConversionError(
                    "points field has no value".to_string(),
                ));
            }
        };

        // Convert each point
        let mut ros_points = Vec::new();
        for point_value in points_array {
            let point_kv = match point_value {
                Value::KeyValue(kv) => kv,
                _ => {
                    return Err(ROS2RobotError::ConversionError(
                        "Expected KeyValue for each trajectory point".to_string(),
                    ));
                }
            };

            // Extract positions
            let positions = extract_f64_array(point_kv, "positions")?;

            // Extract timeFromStart
            let time_from_start = extract_f64(point_kv, "timeFromStart")?;

            let secs = time_from_start.trunc() as i32;
            let nanosecs = (time_from_start.fract() * 1e9) as u32;

            // Extract optional velocities, accelerations, effort
            let velocities = extract_f64_array(point_kv, "velocities").unwrap_or_default();
            let accelerations = extract_f64_array(point_kv, "accelerations").unwrap_or_default();
            let effort = extract_f64_array(point_kv, "effort").unwrap_or_default();

            ros_points.push(JointTrajectoryPoint {
                positions,
                velocities,
                accelerations,
                effort,
                time_from_start: Duration {
                    sec: secs,
                    nanosec: nanosecs,
                },
            });
        }

        debug!("Successfully converted StateChange to JointTrajectory");
        Ok(Some(JointTrajectory {
            header: Header {
                stamp: get_now(),
                frame_id: "".to_string(),
            },
            joint_names,
            points: ros_points,
        }))
    }
}

/// Helper function to extract an f64 array from a KeyValue field
fn extract_f64_array(kv: &KeyValue, field_name: &str) -> Result<Vec<f64>, ROS2RobotError> {
    let field = kv.fields.get(field_name).ok_or_else(|| {
        ROS2RobotError::ConversionError(format!("Missing '{}' field", field_name))
    })?;

    match &field.value {
        Some(boxed_value) => match boxed_value.as_ref() {
            Value::ArrayF64(arr) => Ok(arr.clone()),
            Value::ArrayF32(arr) => Ok(arr.iter().map(|&x| x as f64).collect()),
            _ => Err(ROS2RobotError::ConversionError(format!(
                "Expected ArrayF64 for {}",
                field_name
            ))),
        },
        None => Err(ROS2RobotError::ConversionError(format!(
            "{} field has no value",
            field_name
        ))),
    }
}

impl FromStateChange for msgs::String {
    fn from_state_change(
        state_change: &StateChange,
        _joint_ids_to_ros_names: &HashMap<String, String>,
    ) -> Result<Option<Self>, ROS2RobotError> {
        debug!("Converting StateChange to String message");

        // Look for "data" key in state change
        let data_key = Key::from("data".to_string());

        match state_change.set.get(&data_key) {
            Some(Some(Value::String(s))) => {
                debug!(
                    "Successfully converted StateChange to String message: {}",
                    s
                );
                Ok(Some(msgs::String { data: s.clone() }))
            }
            Some(Some(value)) => Err(ROS2RobotError::ConversionError(format!(
                "Expected string value for 'data' key, got: {:?}",
                value
            ))),
            Some(None) => Err(ROS2RobotError::ConversionError(
                "'data' key has no value".to_string(),
            )),
            None => Ok(None),
        }
    }
}

/// Helper function to extract an f64 from a KeyValue field
fn extract_f64(kv: &KeyValue, field_name: &str) -> Result<f64, ROS2RobotError> {
    let field = kv.fields.get(field_name).ok_or_else(|| {
        ROS2RobotError::ConversionError(format!("Missing '{}' field", field_name))
    })?;

    match &field.value {
        Some(boxed_value) => match boxed_value.as_ref() {
            Value::F64(val) => Ok(*val),
            Value::F32(val) => Ok(*val as f64),
            Value::I64(val) => Ok(*val as f64),
            Value::I32(val) => Ok(*val as f64),
            _ => Err(ROS2RobotError::ConversionError(format!(
                "Expected numeric value for {}",
                field_name
            ))),
        },
        None => Err(ROS2RobotError::ConversionError(format!(
            "{} field has no value",
            field_name
        ))),
    }
}
