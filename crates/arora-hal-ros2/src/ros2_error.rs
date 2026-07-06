use thiserror::Error;

/// Error type for the ROS2 robot HAL
#[derive(Error, Debug)]
pub enum ROS2RobotError {
    /// Error related to configuration
    #[error("Configuration error: {0}")]
    ConfigError(String),

    /// Error when message type is not supported
    #[error("Unsupported message type: {0}")]
    UnsupportedMessageType(String),

    /// Failed to initialize ROS2 context.
    #[error("Failed to initialize ROS2 context: {0}")]
    InitializationError(String),

    /// Topic not found error
    #[error("Topic not found: {0}")]
    TopicNotFound(String),

    #[error("Error from publisher for '{topic}': {reason}")]
    PublisherError {
        /// The topic name.
        topic: String,
        /// The reason for the failure.
        reason: String,
    },

    #[error("Error from subscriber for '{topic}': {reason}")]
    SubscriberError {
        /// The topic name.
        topic: String,
        /// The reason for the failure.
        reason: String,
    },

    /// Error when converting between message formats
    #[error("Conversion error: {0}")]
    ConversionError(String),

    /// Other error
    #[error("Other error: {0}")]
    Other(String),
}

impl From<std::io::Error> for ROS2RobotError {
    fn from(err: std::io::Error) -> Self {
        Self::Other(format!("IO error: {}", err))
    }
}

impl From<serde_json::Error> for ROS2RobotError {
    fn from(err: serde_json::Error) -> Self {
        Self::ConfigError(format!("JSON error: {}", err))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    #[test]
    fn test_config_error_display() {
        let error = ROS2RobotError::ConfigError("Test config error".to_string());
        assert_eq!(error.to_string(), "Configuration error: Test config error");
    }

    #[test]
    fn test_unsupported_message_type_display() {
        let error = ROS2RobotError::UnsupportedMessageType("test_msgs/TestMessage".to_string());
        assert_eq!(
            error.to_string(),
            "Unsupported message type: test_msgs/TestMessage"
        );
    }

    #[test]
    fn test_conversion_error_display() {
        let error = ROS2RobotError::ConversionError("failed".to_string());
        assert_eq!(error.to_string(), "Conversion error: failed");
    }

    #[test]
    fn test_topic_not_found_display() {
        let error = ROS2RobotError::TopicNotFound("/test_topic".to_string());
        assert_eq!(error.to_string(), "Topic not found: /test_topic");
    }

    #[test]
    fn test_other_error_display() {
        let error = ROS2RobotError::Other("Some other error".to_string());
        assert_eq!(error.to_string(), "Other error: Some other error");
    }

    #[test]
    fn test_from_io_error() {
        let io_error = io::Error::new(io::ErrorKind::NotFound, "File not found");
        let ros2_error = ROS2RobotError::from(io_error);

        match ros2_error {
            ROS2RobotError::Other(msg) => {
                assert!(msg.contains("IO error"));
                assert!(msg.contains("File not found"));
            }
            _ => panic!("Expected Other error variant"),
        }
    }

    #[test]
    fn test_from_serde_json_error() {
        let json_error = serde_json::from_str::<serde_json::Value>("invalid json").unwrap_err();
        let ros2_error = ROS2RobotError::from(json_error);

        match ros2_error {
            ROS2RobotError::ConfigError(msg) => {
                assert!(msg.contains("JSON error"));
            }
            _ => panic!("Expected ConfigError variant"),
        }
    }

    #[test]
    fn test_error_is_send_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}

        assert_send::<ROS2RobotError>();
        assert_sync::<ROS2RobotError>();
    }

    #[test]
    fn test_error_debug() {
        let error = ROS2RobotError::ConfigError("test".to_string());
        let debug_str = format!("{error:?}");
        assert!(debug_str.contains("ConfigError"));
        assert!(debug_str.contains("test"));
    }
}
