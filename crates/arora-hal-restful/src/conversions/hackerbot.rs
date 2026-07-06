use std::collections::HashMap;

use log::{debug, warn};
use serde_json::{json, Value as JsonValue};

use crate::config::{EndpointConfig, EndpointMapping};

/// Convert joint positions and velocities into a JSON payload for a hackerbot API call.
pub(crate) fn convert_joint_positions_to_request(
    joints: &[String],
    angles: &[f64],
    velocities: &[f64],
    endpoint: &EndpointConfig,
) -> Option<JsonValue> {
    debug!(
        "Converting joint positions to hackerbot API request: joints={:?}, angles={:?}, velocities={:?} for endpoint {:?}",
        joints, angles, velocities, endpoint
    );

    match endpoint.mapping.as_ref() {
        Some(EndpointMapping::JointPositions { joint_mapping }) => {
            convert_joint_positions_and_velocities(
                joints,
                angles,
                velocities,
                joint_mapping,
                &endpoint.path,
            )
        }
        Some(EndpointMapping::Twist) => {
            // TODO: Implement conversion for Twist commands
            None
        }
        Some(EndpointMapping::Navigation) => {
            // TODO: Implement conversion for Navigation commands
            None
        }
        Some(EndpointMapping::Custom { field_mappings: _ }) => {
            // TODO: Implement custom field mapping
            None
        }
        None => None,
    }
}

fn convert_joint_positions_and_velocities(
    joints: &[String],
    angles: &[f64],
    velocities: &[f64],
    joint_mapping: &HashMap<String, String>,
    endpoint_path: &str,
) -> Option<JsonValue> {
    if joints.len() != angles.len() || joints.len() != velocities.len() {
        warn!("Mismatch in joints, angles, or velocities length for conversion");
        return None;
    }

    let normalized_path = crate::utils::normalize_endpoint_path(endpoint_path);

    if normalized_path.ends_with("/api/v1/head") {
        let mut head_payload = serde_json::Map::new();
        head_payload.insert("method".to_string(), json!("look"));

        let mut joint_data = HashMap::new();
        for (i, joint_name) in joints.iter().enumerate() {
            if let Some(api_name) = joint_mapping.get(joint_name) {
                joint_data.insert(api_name.clone(), (angles[i], velocities[i]));
            }
        }

        let (yaw_angle, yaw_velocity) = joint_data.get("yaw").cloned().unwrap_or((0.0, 0.0));
        let (pitch_angle, pitch_velocity) = joint_data.get("pitch").cloned().unwrap_or((0.0, 0.0));

        let head_velocity_scalar = 75.0;

        let max_velocity = yaw_velocity.max(pitch_velocity) * head_velocity_scalar;

        // Convert radians to degrees for the API
        let urdf_adj = 180.0; // temp adjustment until URDF matches expected range in studio
        let yaw_deg = yaw_angle.to_degrees() + urdf_adj;
        let pitch_deg = pitch_angle.to_degrees() + urdf_adj;

        head_payload.insert("yaw".to_string(), json!(yaw_deg));
        head_payload.insert("pitch".to_string(), json!(pitch_deg));
        head_payload.insert("speed".to_string(), json!(max_velocity));

        Some(JsonValue::Object(head_payload))
    } else if normalized_path.ends_with("/api/v1/arm") {
        let mut joint_angle_map: HashMap<_, _> =
            joints.iter().cloned().zip(angles.iter().cloned()).collect();

        let mut ordered_angles_rad = [0.0; 6];
        let mut joints_found = 0;

        for i in 1..=6 {
            let joint_name = format!("joint{}", i);
            if let Some(angle) = joint_angle_map.remove(&joint_name) {
                ordered_angles_rad[i - 1] = angle;
                joints_found += 1;
            }
        }

        if joints_found == 0 {
            warn!("No valid arm joints found for arm conversion");
            return None;
        }

        let max_velocity = velocities
            .iter()
            .fold(0.0, |max, &val| if val > max { val } else { max });

        let arm_velocity_scalar = 40.0;

        let ordered_angles_deg: Vec<f64> =
            ordered_angles_rad.iter().map(|r| r.to_degrees()).collect();

        Some(json!({
            "method": "move-joints",
            "angles": ordered_angles_deg,
            "speed": max_velocity * arm_velocity_scalar
        }))
    } else {
        warn!(
            "Unknown endpoint path for joint positions conversion: {}",
            endpoint_path
        );
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::Method;

    fn head_endpoint() -> EndpointConfig {
        EndpointConfig {
            path: "/api/v1/head".to_string(),
            method: Method::POST,
            mapping: Some(EndpointMapping::JointPositions {
                joint_mapping: HashMap::from([
                    ("head_rotation".to_string(), "yaw".to_string()),
                    ("head_vert".to_string(), "pitch".to_string()),
                ]),
            }),
        }
    }

    fn arm_endpoint() -> EndpointConfig {
        EndpointConfig {
            path: "/api/v1/arm".to_string(),
            method: Method::POST,
            mapping: Some(EndpointMapping::JointPositions {
                joint_mapping: (1..=6)
                    .map(|i| (format!("joint{i}"), format!("joint{i}")))
                    .collect(),
            }),
        }
    }

    #[test]
    fn test_head_payload_converts_radians_to_degrees() {
        let payload = convert_joint_positions_to_request(
            &["head_rotation".to_string(), "head_vert".to_string()],
            &[0.5, -0.2],
            &[1.0, 1.0],
            &head_endpoint(),
        )
        .expect("head conversion should produce a payload");

        assert_eq!(payload["method"], json!("look"));
        let yaw = payload["yaw"].as_f64().unwrap();
        let pitch = payload["pitch"].as_f64().unwrap();
        assert!((yaw - (0.5f64.to_degrees() + 180.0)).abs() < 1e-9);
        assert!((pitch - ((-0.2f64).to_degrees() + 180.0)).abs() < 1e-9);
        assert_eq!(payload["speed"].as_f64().unwrap(), 75.0);
    }

    #[test]
    fn test_arm_payload_orders_joints() {
        let payload = convert_joint_positions_to_request(
            &["joint2".to_string(), "joint1".to_string()],
            &[0.2, 0.1],
            &[1.0, 1.0],
            &arm_endpoint(),
        )
        .expect("arm conversion should produce a payload");

        assert_eq!(payload["method"], json!("move-joints"));
        let angles: Vec<f64> = payload["angles"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect();
        assert!((angles[0] - 0.1f64.to_degrees()).abs() < 1e-9);
        assert!((angles[1] - 0.2f64.to_degrees()).abs() < 1e-9);
        assert_eq!(angles[2..], [0.0, 0.0, 0.0, 0.0]);
        assert_eq!(payload["speed"].as_f64().unwrap(), 40.0);
    }

    #[test]
    fn test_length_mismatch_yields_no_payload() {
        let payload = convert_joint_positions_to_request(
            &["head_rotation".to_string()],
            &[0.5, 0.6],
            &[1.0],
            &head_endpoint(),
        );
        assert!(payload.is_none());
    }

    #[test]
    fn test_unmapped_endpoint_yields_no_payload() {
        let endpoint = EndpointConfig {
            path: "/api/v1/base/actions".to_string(),
            method: Method::POST,
            mapping: Some(EndpointMapping::Twist),
        };
        let payload = convert_joint_positions_to_request(
            &["head_rotation".to_string()],
            &[0.5],
            &[1.0],
            &endpoint,
        );
        assert!(payload.is_none());
    }
}
