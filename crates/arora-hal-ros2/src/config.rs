use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read};

use byteorder::{LittleEndian, ReadBytesExt};
use serde::{Deserialize, Serialize};

use crate::msgs::RosMessage;
use crate::ros2_error::ROS2RobotError;

/// Represents the complete configuration for a ROS2 robot.
// The default configurations for well-known robots are in `configs` mod.
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct ROS2RobotConfig {
    /// The robot's model family (e.g. "nao"), reported in the HAL description.
    #[serde(default)]
    pub model_family: Option<String>,
    /// The robot's hardware version, reported in the HAL description.
    #[serde(default)]
    pub hardware_version: Option<String>,
    /// The robot's software version, reported in the HAL description.
    #[serde(default)]
    pub software_version: Option<String>,
    /// The ROS2 domain ID for this robot.
    pub domain_id: Option<u16>,
    /// A list of topic configurations for the robot.
    #[serde(default)]
    pub topics: Vec<TopicConfig>,
    /// Associate ROS joint names to Arora IDs (usually found in the robot model)
    #[serde(default)]
    pub joint_ids: JointIdMapping,
    /// Path to the GLB model file for this robot.
    #[serde(default)]
    pub model_glb_path: Option<String>,
}

impl ROS2RobotConfig {
    /// Validate a robot configuration
    pub fn validate(&self) -> Result<(), ROS2RobotError> {
        // Validate that there is at least one topic
        if self.topics.is_empty() {
            return Err(ROS2RobotError::ConfigError(
                "Configuration must have at least one topic".to_string(),
            ));
        }

        // Validate each topic configuration
        for (i, topic) in self.topics.iter().enumerate() {
            if topic.name.is_empty() {
                return Err(ROS2RobotError::ConfigError(format!(
                    "Topic #{} has an empty name",
                    i + 1
                )));
            }

            if topic.message_type.is_empty() {
                return Err(ROS2RobotError::ConfigError(format!(
                    "Topic '{}' has an empty message type",
                    topic.name
                )));
            }
        }

        Ok(())
    }

    /// Apply the fields an overrides file sets.
    ///
    /// A field the overrides leave unset keeps its value: a `None` option, an
    /// empty topic list, and the default `JointIdMapping::FromGLB`. Thus an
    /// overrides file can change the joint mapping to `Override` or `Extend`,
    /// but not back to `FromGLB`.
    pub fn apply_overrides(&mut self, overrides: ROS2RobotConfig) {
        if let Some(domain_id) = overrides.domain_id {
            self.domain_id = Some(domain_id);
        }

        if !overrides.topics.is_empty() {
            self.topics = overrides.topics;
        }

        if overrides.model_family.is_some() {
            self.model_family = overrides.model_family;
        }
        if overrides.hardware_version.is_some() {
            self.hardware_version = overrides.hardware_version;
        }
        if overrides.software_version.is_some() {
            self.software_version = overrides.software_version;
        }
        if overrides.model_glb_path.is_some() {
            self.model_glb_path = overrides.model_glb_path;
        }
        if !matches!(overrides.joint_ids, JointIdMapping::FromGLB) {
            self.joint_ids = overrides.joint_ids;
        }
    }
}

/// Configuration for a single ROS2 topic.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TopicConfig {
    /// The name of the ROS2 topic (e.g., "/joint_states").
    pub name: String,
    /// The ROS2 message type for the topic (e.g., "sensor_msgs/JointState").
    pub message_type: String,
    /// The direction of data flow for this topic (Publish, Subscribe, or Both).
    pub direction: TopicDirection,
    /// Defines how the ROS2 message data is mapped to Arora keys/values.
    pub mapping: TopicMapping,
}

impl TopicConfig {
    pub fn new<T: RosMessage>(
        name: &str,
        direction: TopicDirection,
        mapping: TopicMapping,
    ) -> Self {
        TopicConfig {
            name: name.to_string(),
            // The short `package/Type` form the configs use; `package_and_type`
            // resolves it back to `T` for the typed dispatch in `ros2_hal`.
            message_type: format!("{}/{}", T::PACKAGE, T::TYPE_NAME),
            direction,
            mapping,
        }
    }
}

/// Specifies the direction of data flow for a topic.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum TopicDirection {
    /// The HAL will publish messages to this topic.
    Publish,
    /// The HAL will subscribe to messages from this topic.
    Subscribe,
    /// The HAL will both publish and subscribe to this topic.
    Both,
}

/// Defines how data from a ROS2 message is mapped to Arora keys/values.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum TopicMapping {
    /// Maps a ROS2 JointState message to Arora state keys.
    JointState {
        /// Specifies any necessary conversions for the joint state data.
        conversion: JointStateConversion,
    },
    /// Maps fields from a standard ROS2 message type to Arora state keys based on provided mappings.
    StandardMessage {
        /// A map from ROS2 message field names to Arora state keys.
        field_mappings: HashMap<String, String>,
    },
    /// Maps a ROS2 JointTrajectory message to Arora joint trajectory commands.
    JointTrajectory,
    /// Maps a ROS2 JointAnglesWithSpeed message to Arora joint angle commands.
    JointAngles,
    /// Maps a ROS2 Float64MultiArray message to Arora keys/values.
    Float64MultiArray,
}

/// Specifies conversions to apply to JointState data.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum JointStateConversion {
    /// Standard mapping of JointState fields.
    Standard,
    /// Converts JointState data into a Float64MultiArray format.
    ToMultiArray,
}

/// Mapping options between Arora key IDs and ROS2 joint names.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub enum JointIdMapping {
    /// Use the joint names as defined in the robot's GLB model file.
    #[default]
    FromGLB,
    /// Use a custom mapping, overriding GLB's.
    Override(HashMap<String, String>),
    /// Use a custom mapping, extending GLB's.
    Extend(HashMap<String, String>),
}

/// The error for a robot model file that does not exist. It names the two ways
/// to supply one, because no build step fills the default path.
pub(crate) fn missing_model_message(path: &str) -> String {
    format!(
        "robot model {path} does not exist. Put the robot's GLB file at this path, \
         or set `model_glb_path` in a robot config or an overrides file \
         (`arora-ros2 <robot> <overrides.json>`)"
    )
}

/// Extract joint ID mappings from a GLB file.
pub(crate) fn get_joint_ids_from_glb_file(
    path: &str,
) -> Result<HashMap<String, String>, ROS2RobotError> {
    let file = File::open(path).map_err(|e| {
        ROS2RobotError::ConfigError(if e.kind() == std::io::ErrorKind::NotFound {
            missing_model_message(path)
        } else {
            format!("Failed to open {path}: {e}")
        })
    })?;
    let mut reader = BufReader::new(file);

    let mut magic = [0; 4];
    reader.read_exact(&mut magic)?;
    if &magic != b"glTF" {
        return Err(ROS2RobotError::ConfigError(format!(
            "{path} is not a glb file"
        )));
    }

    let version = reader.read_u32::<LittleEndian>()?;
    let _length = reader.read_u32::<LittleEndian>()?;
    if version != 2 {
        return Err(ROS2RobotError::ConfigError(
            "Only glb version 2 supported".to_string(),
        ));
    }

    let chunk_length = reader.read_u32::<LittleEndian>()?;
    let chunk_type = reader.read_u32::<LittleEndian>()?;
    if chunk_type != 0x4E4F534A {
        return Err(ROS2RobotError::ConfigError(
            "Invalid first glb chunk".to_string(),
        ));
    }

    let mut joint_ids = HashMap::new();

    let mut json = vec![0; chunk_length as usize];
    reader.read_exact(&mut json[..])?;
    let json: serde_json::Value = serde_json::from_slice(json.as_slice())?;
    let nodes = json
        .get("nodes")
        .ok_or_else(|| {
            ROS2RobotError::ConfigError("Missing nodes section in GLB JSON data".to_string())
        })?
        .as_array()
        .ok_or_else(|| ROS2RobotError::ConfigError("Nodes section is not an array".to_string()))?;
    for node in nodes {
        // Joints are the nodes with an animated RobotData jointValue feature.
        let Some(joint_value) = node
            .get("extensions")
            .and_then(|extensions| extensions.get("RobotData"))
            .and_then(|robot_data| robot_data.get("features"))
            .and_then(|features| features.get("jointValue"))
        else {
            continue;
        };
        let animated = joint_value
            .get("animated")
            .and_then(|animated| animated.as_bool())
            .unwrap_or(false);
        if !animated {
            continue;
        }
        let Some(value) = joint_value.get("value") else {
            continue;
        };
        let (Some(joint_name), Some(joint_id)) = (value.get("name"), value.get("id")) else {
            continue;
        };
        joint_ids.insert(
            joint_name
                .as_str()
                .ok_or_else(|| {
                    ROS2RobotError::ConfigError("Joint name is not a string".to_string())
                })?
                .to_string(),
            joint_id
                .as_str()
                .ok_or_else(|| ROS2RobotError::ConfigError("Joint id is not a string".to_string()))?
                .to_string(),
        );
    }
    Ok(joint_ids)
}

#[cfg(test)]
mod tests {
    use crate::test_fixture::{fixture_glb_path, FIXTURE_JOINTS};

    use super::*;
    use std::{collections::HashMap, io::Write};

    fn create_test_config() -> ROS2RobotConfig {
        ROS2RobotConfig {
            domain_id: Some(42),
            topics: vec![TopicConfig {
                name: "/joint_states".to_string(),
                message_type: "sensor_msgs/JointState".to_string(),
                direction: TopicDirection::Subscribe,
                mapping: TopicMapping::JointState {
                    conversion: JointStateConversion::Standard,
                },
            }],
            ..Default::default()
        }
    }

    #[test]
    fn test_valid_config_validates() {
        let config = create_test_config();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_no_topics_fails_validation() {
        let mut config = create_test_config();
        config.topics.clear();

        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("must have at least one topic"));
    }

    #[test]
    fn test_empty_topic_name_fails_validation() {
        let mut config = create_test_config();
        config.topics[0].name = String::new();

        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("has an empty name"));
    }

    #[test]
    fn test_empty_message_type_fails_validation() {
        let mut config = create_test_config();
        config.topics[0].message_type = String::new();

        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("has an empty message type"));
    }

    #[test]
    fn test_apply_overrides_model() {
        let mut config = create_test_config();
        let override_config = ROS2RobotConfig::default();

        config.apply_overrides(override_config);

        assert_eq!(config.domain_id, Some(42)); // Should remain unchanged
        assert_eq!(config.topics.len(), 1); // Should remain unchanged
    }

    #[test]
    fn test_apply_overrides_domain_id() {
        let mut config = create_test_config();
        let override_config = ROS2RobotConfig {
            domain_id: Some(99),
            topics: vec![],
            ..Default::default()
        };

        config.apply_overrides(override_config);

        assert_eq!(config.domain_id, Some(99));
    }

    #[test]
    fn test_apply_overrides_topics() {
        let mut config = create_test_config();
        let new_topic = TopicConfig {
            name: "/new_topic".to_string(),
            message_type: "std_msgs/String".to_string(),
            direction: TopicDirection::Both,
            mapping: TopicMapping::StandardMessage {
                field_mappings: HashMap::new(),
            },
        };
        let override_config = ROS2RobotConfig {
            topics: vec![new_topic.clone()],
            ..Default::default()
        };

        config.apply_overrides(override_config);

        assert_eq!(config.topics.len(), 1);
        assert_eq!(config.topics[0].name, "/new_topic");
    }

    #[test]
    fn test_apply_overrides_no_change_when_default() {
        let mut config = create_test_config();
        let original_domain_id = config.domain_id;
        let original_topics_len = config.topics.len();

        let override_config = ROS2RobotConfig::default();

        config.apply_overrides(override_config);

        assert_eq!(config.domain_id, original_domain_id);
        assert_eq!(config.topics.len(), original_topics_len);
    }

    #[test]
    fn test_apply_overrides_description_fields() {
        let mut config = create_test_config();
        config.model_family = Some("nao".to_string());
        config.hardware_version = Some("v5".to_string());

        let overrides = ROS2RobotConfig {
            model_family: Some("nao6".to_string()),
            software_version: Some("2.8".to_string()),
            ..Default::default()
        };
        config.apply_overrides(overrides);

        assert_eq!(config.model_family.as_deref(), Some("nao6"));
        assert_eq!(config.software_version.as_deref(), Some("2.8"));
        // A None override preserves the existing value.
        assert_eq!(config.hardware_version.as_deref(), Some("v5"));
    }

    #[test]
    fn test_built_in_configs_set_model_family() {
        for (name, config) in [
            ("nao", crate::configs::nao::create_config()),
            ("pepper", crate::configs::pepper::create_config()),
            ("quori", crate::configs::quori::create_config()),
            ("ur3", crate::configs::ur3::create_config()),
            ("ur5", crate::configs::ur5::create_config()),
            ("g1", crate::configs::unitree_g1::create_config()),
        ] {
            assert!(
                config.model_family.is_some(),
                "{name} config must set model_family for Hal::describe()"
            );
        }
    }

    #[test]
    fn test_serde_round_trip() {
        let config = create_test_config();

        // Serialize to JSON
        let json = serde_json::to_string(&config).expect("Failed to serialize config");

        // Deserialize back
        let deserialized: ROS2RobotConfig =
            serde_json::from_str(&json).expect("Failed to deserialize config");

        // Verify they match
        assert_eq!(config.domain_id, deserialized.domain_id);
        assert_eq!(config.topics.len(), deserialized.topics.len());
        assert_eq!(config.topics[0].name, deserialized.topics[0].name);
    }

    #[test]
    fn test_apply_overrides_model_glb_path() {
        let mut config = create_test_config();
        config.model_glb_path = Some("/default/nao.glb".to_string());

        config.apply_overrides(ROS2RobotConfig {
            model_glb_path: Some("/local/nao.glb".to_string()),
            ..Default::default()
        });
        assert_eq!(config.model_glb_path.as_deref(), Some("/local/nao.glb"));

        // An override without a model path keeps the existing one.
        config.apply_overrides(ROS2RobotConfig::default());
        assert_eq!(config.model_glb_path.as_deref(), Some("/local/nao.glb"));
    }

    #[test]
    fn test_apply_overrides_joint_ids() {
        let mut config = create_test_config();
        let mapping = HashMap::from([("HeadYaw".to_string(), "head-yaw-id".to_string())]);

        config.apply_overrides(ROS2RobotConfig {
            joint_ids: JointIdMapping::Extend(mapping.clone()),
            ..Default::default()
        });
        assert!(matches!(&config.joint_ids, JointIdMapping::Extend(m) if *m == mapping));

        // The default `FromGLB` reads as "not set", so the mapping stays.
        config.apply_overrides(ROS2RobotConfig::default());
        assert!(matches!(&config.joint_ids, JointIdMapping::Extend(m) if *m == mapping));
    }

    #[test]
    fn test_apply_overrides_from_json_overrides_file() {
        // The shape of an overrides file the runner reads after a built-in robot.
        let overrides: ROS2RobotConfig =
            serde_json::from_str(r#"{ "model_glb_path": "/local/quori.glb" }"#)
                .expect("an overrides file with only a model path should parse");
        let mut config = crate::configs::quori::create_config();
        let topics_before = config.topics.len();

        config.apply_overrides(overrides);

        assert_eq!(config.model_glb_path.as_deref(), Some("/local/quori.glb"));
        assert_eq!(config.topics.len(), topics_before);
        assert!(matches!(config.joint_ids, JointIdMapping::FromGLB));
    }

    #[test]
    fn test_get_joint_ids_from_glb_file() {
        let joint_ids =
            get_joint_ids_from_glb_file(&fixture_glb_path()).expect("the fixture GLB should parse");

        // Only the animated joints: the plain node and the joint that is not
        // animated are skipped.
        let expected: HashMap<String, String> = FIXTURE_JOINTS
            .iter()
            .map(|(name, id)| (name.to_string(), id.to_string()))
            .collect();
        assert_eq!(joint_ids, expected);
    }

    #[test]
    fn test_get_joint_ids_from_invalid_file() {
        let result = get_joint_ids_from_glb_file("nonexistent_file.glb");
        assert!(result.is_err(), "Should fail on nonexistent file");
    }

    #[test]
    fn test_missing_model_error_names_both_remedies() {
        let err = get_joint_ids_from_glb_file("/no/such/dir/nao.glb")
            .expect_err("a missing model file is an error")
            .to_string();
        assert!(err.contains("/no/such/dir/nao.glb does not exist"), "{err}");
        assert!(
            err.contains("Put the robot's GLB file at this path"),
            "{err}"
        );
        assert!(err.contains("model_glb_path"), "{err}");
    }

    #[test]
    fn test_get_joint_ids_from_non_glb_file() {
        let temp_file = std::env::temp_dir().join("test_invalid.glb");
        let mut file = File::create(&temp_file).unwrap();
        file.write_all(b"Not a GLB file").unwrap();

        let result = get_joint_ids_from_glb_file(temp_file.to_str().unwrap());
        assert!(result.is_err(), "Should fail on non-GLB file");

        std::fs::remove_file(temp_file).ok();
    }
}
