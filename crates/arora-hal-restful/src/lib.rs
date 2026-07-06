//! RESTful-API robots as Arora HALs.
//!
//! One [`arora_hal::Hal`] implementation, [`RestfulHal`], drives any robot
//! controlled over a local HTTP API (such as the hackerbot) from a
//! [`RESTfulRobotConfig`]: API endpoints map to Arora keys/values, and the
//! configuration decides which robot it is. There are no built-in robots; a
//! JSON file deserializes to [`RESTfulRobotConfig`] (see
//! `configs/hackerbot.json` for a sample).
//!
//! Keys follow hierarchical paths like `head_yaw.target_position`; values are
//! from the arora-types [`Value`](arora_types::value::Value) enum; state
//! changes are applied as sets and unsets.

mod config;
pub use config::{EndpointConfig, EndpointMapping, RESTfulRobotConfig};

mod conversions;

mod restful_error;
pub use restful_error::RESTfulRobotError;

mod restful_hal;
pub use restful_hal::RestfulHal;

mod tls;
mod utils;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_exports() {
        // Test that important types are exported
        let _config = RESTfulRobotConfig::default();
        let _error: RESTfulRobotError = RESTfulRobotError::ConfigError("test".to_string());

        // Test that config enums are exported
        let _mapping = EndpointMapping::Twist;
    }

    #[test]
    fn test_hackerbot_sample_config_parses() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/configs/hackerbot.json");
        let file = std::fs::File::open(path).expect("hackerbot sample config should exist");
        let config: RESTfulRobotConfig =
            serde_json::from_reader(file).expect("hackerbot sample config should parse");
        assert!(config.validate().is_ok());
        assert_eq!(
            config.model_family.as_deref(),
            Some("hackerbot"),
            "the sample config must set model_family for Hal::describe()"
        );
        assert!(!config.endpoints.is_empty());
    }
}
