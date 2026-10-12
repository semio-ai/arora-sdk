use arora_hal::HalError;

/// Errors of the NAOqi HAL.
#[derive(Debug, thiserror::Error)]
pub enum NaoqiRobotError {
    /// The configuration is invalid.
    #[error("configuration error: {0}")]
    Config(String),
    /// The robot could not be reached, or a service it must expose is missing.
    #[error("connection to NAOqi failed: {0}")]
    Connection(String),
    /// A call to a NAOqi service failed.
    #[error("NAOqi call failed: {0}")]
    Qi(#[from] qi::Error),
    /// A value could not be converted between Arora and NAOqi representations.
    #[error("conversion error: {0}")]
    Conversion(String),
    /// Every NAOqi command of a write failed.
    #[error("write failed: {0}")]
    Write(String),
}

impl From<NaoqiRobotError> for HalError {
    fn from(error: NaoqiRobotError) -> Self {
        match error {
            NaoqiRobotError::Connection(message) => HalError::Broken(message),
            other => HalError::Other(other.to_string()),
        }
    }
}
