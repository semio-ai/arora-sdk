//! The answer to an `invoke`.

use arora_types::value::Value;

/// Result type for method invocation.
#[derive(Debug, Clone)]
pub struct InvokeResult {
    pub success: bool,
    pub value: Option<Value>,
    pub message: Option<String>,
}

impl InvokeResult {
    /// Create a successful result with no return value.
    pub fn ok() -> Self {
        Self {
            success: true,
            value: None,
            message: None,
        }
    }

    /// Create a successful result with a return value.
    pub fn ok_with_value(value: Value) -> Self {
        Self {
            success: true,
            value: Some(value),
            message: None,
        }
    }

    /// Create an error result.
    pub fn err(message: impl Into<String>) -> Self {
        Self {
            success: false,
            value: None,
            message: Some(message.into()),
        }
    }
}
