use std::collections::HashMap;
use std::str::FromStr;

use reqwest::Method;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::restful_error::RESTfulRobotError;

/// Represents the complete configuration for a RESTful API robot.
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct RESTfulRobotConfig {
    /// The robot's model family (e.g. "hackerbot"), reported in the HAL description.
    #[serde(default)]
    pub model_family: Option<String>,
    /// The robot's hardware version, reported in the HAL description.
    #[serde(default)]
    pub hardware_version: Option<String>,
    /// The robot's software version, reported in the HAL description.
    #[serde(default)]
    pub software_version: Option<String>,
    /// The base URL for the robot's API (e.g., "http://<host>").
    pub base_url: String,
    /// A list of endpoint configurations for the robot.
    #[serde(default)]
    pub endpoints: Vec<EndpointConfig>,
}

impl RESTfulRobotConfig {
    /// Validate a RESTful API robot configuration.
    pub fn validate(&self) -> Result<(), RESTfulRobotError> {
        // Validate that the base URL is not empty
        if self.base_url.is_empty() {
            return Err(RESTfulRobotError::ConfigError(
                "Base URL cannot be empty".to_string(),
            ));
        }

        // Validate that there is at least one endpoint
        if self.endpoints.is_empty() {
            return Err(RESTfulRobotError::ConfigError(
                "Configuration must have at least one endpoint".to_string(),
            ));
        }

        // Validate each endpoint configuration
        for (i, endpoint) in self.endpoints.iter().enumerate() {
            if endpoint.path.is_empty() {
                return Err(RESTfulRobotError::ConfigError(format!(
                    "Endpoint #{} has an empty path",
                    i + 1
                )));
            }

            if endpoint.mapping.is_none() {
                return Err(RESTfulRobotError::ConfigError(format!(
                    "Endpoint '{}' does not have a mapping defined",
                    endpoint.path
                )));
            }
        }

        Ok(())
    }

    /// Apply overrides to the current configuration.
    pub fn apply_overrides(&mut self, overrides: RESTfulRobotConfig) {
        if !overrides.base_url.is_empty() {
            self.base_url = overrides.base_url;
        }

        if !overrides.endpoints.is_empty() {
            self.endpoints = overrides.endpoints;
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
    }
}

/// Configuration for a single RESTful API endpoint.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EndpointConfig {
    /// The path of the API endpoint (e.g., "/api/v1/base").
    pub path: String,
    /// The HTTP method to use for this endpoint (e.g., `Method::POST`, `Method::GET`).
    #[serde(
        serialize_with = "serialize_method",
        deserialize_with = "deserialize_method"
    )]
    pub method: Method,
    /// Defines how the API request/response data is mapped to Arora keys/values.
    #[serde(default)]
    pub mapping: Option<EndpointMapping>,
}

fn serialize_method<S>(method: &Method, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(method.as_str())
}

fn deserialize_method<'de, D>(deserializer: D) -> Result<Method, D::Error>
where
    D: Deserializer<'de>,
{
    let s: String = Deserialize::deserialize(deserializer)?;
    Method::from_str(&s).map_err(serde::de::Error::custom)
}

/// Defines how data from a RESTful API endpoint is mapped to Arora keys/values.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum EndpointMapping {
    /// Maps to joint position commands or states.
    JointPositions {
        /// A map from Arora joint IDs to the API's joint names.
        joint_mapping: HashMap<String, String>,
    },
    /// Maps to navigation or pose commands.
    Navigation,
    /// Maps to velocity or twist commands.
    Twist,
    /// Custom mapping for specific API responses to Arora keys.
    Custom {
        /// A map from API response field names to Arora keys.
        field_mappings: HashMap<String, String>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_config() -> RESTfulRobotConfig {
        RESTfulRobotConfig {
            base_url: "http://localhost:5000".to_string(),
            endpoints: vec![
                EndpointConfig {
                    path: "/api/v1/base".to_string(),
                    method: Method::POST,
                    mapping: Some(EndpointMapping::Twist),
                },
                EndpointConfig {
                    path: "/api/v1/arm".to_string(),
                    method: Method::POST,
                    mapping: Some(EndpointMapping::JointPositions {
                        joint_mapping: HashMap::new(),
                    }),
                },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn test_valid_config_validates() {
        let config = create_test_config();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_empty_base_url_fails_validation() {
        let mut config = create_test_config();
        config.base_url = String::new();

        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Base URL cannot be empty"));
    }

    #[test]
    fn test_no_endpoints_fails_validation() {
        let mut config = create_test_config();
        config.endpoints.clear();

        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("must have at least one endpoint"));
    }

    #[test]
    fn test_empty_endpoint_path_fails_validation() {
        let mut config = create_test_config();
        config.endpoints[0].path = String::new();

        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("has an empty path"));
    }

    #[test]
    fn test_no_mapping_fails_validation() {
        let mut config = create_test_config();
        config.endpoints[0].mapping = None;

        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("does not have a mapping defined"));
    }

    #[test]
    fn test_apply_overrides_base_url() {
        let mut config = create_test_config();
        let override_config = RESTfulRobotConfig {
            base_url: "http://newhost:8080".to_string(),
            ..Default::default()
        };

        config.apply_overrides(override_config);

        assert_eq!(config.base_url, "http://newhost:8080");
        assert_eq!(config.endpoints.len(), 2); // Should remain unchanged
    }

    #[test]
    fn test_apply_overrides_endpoints() {
        let mut config = create_test_config();
        let new_endpoint = EndpointConfig {
            path: "/api/v1/new".to_string(),
            method: Method::GET,
            mapping: Some(EndpointMapping::Custom {
                field_mappings: HashMap::new(),
            }),
        };
        let override_config = RESTfulRobotConfig {
            endpoints: vec![new_endpoint.clone()],
            ..Default::default()
        };

        config.apply_overrides(override_config);

        assert_eq!(config.endpoints.len(), 1);
        assert_eq!(config.endpoints[0].path, "/api/v1/new");
        assert_eq!(config.base_url, "http://localhost:5000"); // Should remain unchanged
    }

    #[test]
    fn test_apply_overrides_description_fields() {
        let mut config = create_test_config();
        config.model_family = Some("hackerbot".to_string());
        config.hardware_version = Some("v1".to_string());

        let overrides = RESTfulRobotConfig {
            model_family: Some("hackerbot2".to_string()),
            software_version: Some("0.9".to_string()),
            ..Default::default()
        };
        config.apply_overrides(overrides);

        assert_eq!(config.model_family.as_deref(), Some("hackerbot2"));
        assert_eq!(config.software_version.as_deref(), Some("0.9"));
        // A None override preserves the existing value.
        assert_eq!(config.hardware_version.as_deref(), Some("v1"));
    }

    #[test]
    fn test_describe_fields_parse_from_json() {
        let json = r#"{
            "model_family": "hackerbot",
            "hardware_version": "v1",
            "software_version": "0.9",
            "base_url": "http://localhost:5000",
            "endpoints": [
                { "path": "/api/v1/base", "method": "POST", "mapping": "Twist" }
            ]
        }"#;
        let config: RESTfulRobotConfig = serde_json::from_str(json).expect("config should parse");
        assert_eq!(config.model_family.as_deref(), Some("hackerbot"));
        assert_eq!(config.hardware_version.as_deref(), Some("v1"));
        assert_eq!(config.software_version.as_deref(), Some("0.9"));
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_describe_fields_default_to_none() {
        let json = r#"{
            "base_url": "http://localhost:5000",
            "endpoints": [
                { "path": "/api/v1/base", "method": "POST", "mapping": "Twist" }
            ]
        }"#;
        let config: RESTfulRobotConfig = serde_json::from_str(json).expect("config should parse");
        assert_eq!(config.model_family, None);
        assert_eq!(config.hardware_version, None);
        assert_eq!(config.software_version, None);
    }

    #[test]
    fn test_serde_round_trip() {
        let mut config = create_test_config();
        config.model_family = Some("hackerbot".to_string());

        // Serialize to JSON
        let json = serde_json::to_string(&config).expect("Failed to serialize config");

        // Deserialize back
        let deserialized: RESTfulRobotConfig =
            serde_json::from_str(&json).expect("Failed to deserialize config");

        // Verify they match
        assert_eq!(config.base_url, deserialized.base_url);
        assert_eq!(config.model_family, deserialized.model_family);
        assert_eq!(config.endpoints.len(), deserialized.endpoints.len());
        assert_eq!(config.endpoints[0].path, deserialized.endpoints[0].path);
        assert_eq!(config.endpoints[0].method, deserialized.endpoints[0].method);
    }
}
