use thiserror::Error;

/// Error type for the RESTful API robot HAL
#[derive(Error, Debug)]
pub enum RESTfulRobotError {
    /// Error related to configuration
    #[error("Configuration error: {0}")]
    ConfigError(String),

    /// Error related to HTTP request failures or API connectivity
    #[error("Request error: {0}")]
    RequestError(String),

    /// Error related to parsing API responses
    #[error("Response parse error: {0}")]
    ResponseParseError(String),

    /// Other error
    #[error("Other error: {0}")]
    Other(String),
}

impl From<reqwest::Error> for RESTfulRobotError {
    fn from(err: reqwest::Error) -> Self {
        Self::RequestError(format!("HTTP request failed: {}", err))
    }
}

impl From<serde_json::Error> for RESTfulRobotError {
    fn from(err: serde_json::Error) -> Self {
        Self::ConfigError(format!("JSON error: {}", err))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_error_display() {
        let error = RESTfulRobotError::ConfigError("Test config error".to_string());
        assert_eq!(error.to_string(), "Configuration error: Test config error");
    }

    #[test]
    fn test_request_error_display() {
        let error = RESTfulRobotError::RequestError("connection refused".to_string());
        assert_eq!(error.to_string(), "Request error: connection refused");
    }

    #[test]
    fn test_response_parse_error_display() {
        let error = RESTfulRobotError::ResponseParseError("bad json".to_string());
        assert_eq!(error.to_string(), "Response parse error: bad json");
    }

    #[test]
    fn test_other_error_display() {
        let error = RESTfulRobotError::Other("Some other error".to_string());
        assert_eq!(error.to_string(), "Other error: Some other error");
    }

    #[test]
    fn test_from_serde_json_error() {
        let json_error = serde_json::from_str::<serde_json::Value>("invalid json").unwrap_err();
        let restful_error = RESTfulRobotError::from(json_error);

        match restful_error {
            RESTfulRobotError::ConfigError(msg) => {
                assert!(msg.contains("JSON error"));
            }
            _ => panic!("Expected ConfigError variant"),
        }
    }

    #[test]
    fn test_error_is_send_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}

        assert_send::<RESTfulRobotError>();
        assert_sync::<RESTfulRobotError>();
    }

    #[test]
    fn test_error_debug() {
        let error = RESTfulRobotError::ConfigError("test".to_string());
        let debug_str = format!("{error:?}");
        assert!(debug_str.contains("ConfigError"));
        assert!(debug_str.contains("test"));
    }
}
