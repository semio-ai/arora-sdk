use std::collections::HashMap;

use arora_types::data::{Key, StateChange};
use arora_types::value::Value;
use log::{debug, error};

use crate::msgs::{self, Float64MultiArray, JointAnglesWithSpeed, JointState, JointTrajectory};
use crate::ROS2RobotError;

pub trait ToStateChange {
    /// Converts the ROS2 message into an Arora `StateChange`.
    /// Conversion requires a mapping from ROS joint names to Arora joint IDs.
    fn into_state_change(
        self,
        joint_ids: &HashMap<String, String>,
    ) -> Result<StateChange, ROS2RobotError>;
}

impl ToStateChange for msgs::String {
    fn into_state_change(self, _: &HashMap<String, String>) -> Result<StateChange, ROS2RobotError> {
        debug!("Converting String message to StateChange: {:?}", self.data);
        let mut state_change = StateChange::new();
        let key = Key::from("data".to_string());
        state_change
            .set
            .insert(key, Some(Value::from(self.data.clone())));
        debug!(
            "Successfully converted String message to StateChange with 1 key: data = {}",
            self.data
        );
        Ok(state_change)
    }
}

impl ToStateChange for JointState {
    fn into_state_change(
        self,
        joint_ids: &HashMap<String, String>,
    ) -> Result<StateChange, ROS2RobotError> {
        debug!("Converting JointState message to StateChange");
        if self.name.len() != self.position.len() {
            error!(
                "JointState: Joint names ({}) and positions ({}) arrays have different lengths",
                self.name.len(),
                self.position.len()
            );
            return Err(ROS2RobotError::ConversionError(
                "Joint names and positions arrays have different lengths".to_string(),
            ));
        }

        let mut state_change = StateChange::new();
        if self.name.is_empty() {
            debug!("JointState message has no joint names; returning empty StateChange");
            return Ok(state_change);
        }

        let entities: Vec<&String> = self
            .name
            .iter()
            .map(|joint_name| joint_ids.get(joint_name).unwrap_or(joint_name))
            .collect();

        let mut try_convert = |values: Vec<f64>, component: &str| {
            if values.len() == entities.len() {
                for (entity, value) in entities.iter().zip(values) {
                    let key = Key::from(format!("{entity}.{component}"));
                    state_change.set.insert(key, Some(Value::from(value)));
                }
            } else if !values.is_empty() {
                debug!(
                    "JointState: Skipping {component} conversion; lengths do not match (expected {}, got {})",
                    entities.len(),
                    values.len()
                );
            }
        };

        try_convert(self.position, "position");
        try_convert(self.velocity, "velocity");
        try_convert(self.effort, "effort");

        debug!(
            "Successfully converted JointState to StateChange with {} keys",
            state_change.set.len()
        );
        Ok(state_change)
    }
}

impl ToStateChange for JointTrajectory {
    fn into_state_change(self, _: &HashMap<String, String>) -> Result<StateChange, ROS2RobotError> {
        // TODO: Implement JointTrajectory conversion
        Err(ROS2RobotError::ConversionError(
            "JointTrajectory conversion not yet implemented".to_string(),
        ))
    }
}

impl ToStateChange for JointAnglesWithSpeed {
    fn into_state_change(
        self,
        joint_ids: &HashMap<String, String>,
    ) -> Result<StateChange, ROS2RobotError> {
        debug!("Converting JointAnglesWithSpeed to StateChange");

        let nof_joints = self.joint_names.len();
        let nof_angles = self.joint_angles.len();
        if nof_joints != nof_angles {
            let error_message = format!(
                "JointAnglesWithSpeed conversion: Joint names ({nof_joints}) and angles ({nof_angles}) arrays have different lengths"
            );
            error!("{error_message}");
            return Err(ROS2RobotError::ConversionError(error_message));
        }

        // Create Key/Value pairs for each joint position
        let mut state_change = StateChange::new();
        for (joint_name, position) in self.joint_names.into_iter().zip(self.joint_angles) {
            let entity = joint_ids.get(&joint_name).unwrap_or(&joint_name);
            let key = Key::from(format!("{entity}.position"));
            // Convert f32 angles to f64 for consistency with Arora values
            let value = Some(Value::F64(position as f64));
            state_change.set.insert(key, value);
        }
        debug!("Successfully converted JointAnglesWithSpeed to StateChange");
        Ok(state_change)
    }
}

impl ToStateChange for Float64MultiArray {
    fn into_state_change(
        self,
        joint_ids: &HashMap<String, String>,
    ) -> Result<StateChange, ROS2RobotError> {
        debug!("Converting Float64MultiArray to StateChange");

        // Extract joint names from the dimension labels
        let mut names = Vec::with_capacity(self.layout.dim.len());

        // Sort dimensions by stride (highest to lowest) to maintain the correct order
        let mut dimensions = self.layout.dim.clone();
        dimensions.sort_by_key(|dim| std::cmp::Reverse(dim.stride));

        for dim in &dimensions {
            names.push(dim.label.clone());
        }

        // Use data from the array directly
        let positions = self.data.clone();

        let nof_joints = names.len();
        let nof_angles = positions.len();
        if nof_joints != nof_angles {
            let error_message = format!(
                "Float64MultiArray conversion: Joint names ({nof_joints}) and positions ({nof_angles}) arrays have different lengths"
            );
            error!("{error_message}");
            return Err(ROS2RobotError::ConversionError(error_message));
        }

        let mut state_change = StateChange::new();

        // Convert joint positions to Key/Value pairs
        for (i, name) in names.iter().enumerate() {
            let entity = joint_ids.get(name).unwrap_or(name);
            let key = Key::from(format!("{entity}.position"));
            let value = Value::F64(positions[i]);
            state_change.set.insert(key, Some(value));
        }

        debug!(
            "Successfully converted Float64MultiArray to StateChange with {} joint positions",
            names.len()
        );
        Ok(state_change)
    }
}
