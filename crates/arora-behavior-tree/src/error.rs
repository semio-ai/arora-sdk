use arora::call::CallError;
use arora_schema::value::ConversionError;
use derive_more::Display;
use uuid::Uuid;

#[derive(Display, Debug)]
pub enum BehaviorTreeError {
  /// Error when parsing something, such as a behavior tree description.
  #[display(fmt = "parsing error: {}", message)]
  ParsingError { message: String },

  /// Error in the structure of the behavior tree:
  /// cycles, duplicate nodes, dangling references....
  #[display(fmt = "inconsistent behavior tree: {}", message)]
  InconsistentTreeError { message: String },

  /// Error when client performs a call to a module function.
  CallError(CallError),

  /// Client-side value conversion error.
  ConversionError(ConversionError),

  /// Variable referred in the behavior tree was not found.
  #[display(
    fmt = "variable \"{}\" used by node \"{}\" was not found",
    variable,
    node
  )]
  VariableNotFound { variable: Uuid, node: Uuid },

  #[display(fmt = "node \"{}\", child of node \"{}\" was not found", child, node)]
  ChildNodeNotFound { child: Uuid, node: Uuid },

  #[display(
    fmt = "children were specified for node \"{}\", but it does not accept them as a parameter",
    node
  )]
  MissingChildrenParameter { node: Uuid },

  #[display(fmt = "internal error: {}", message)]
  InternalError { message: String },
}

impl std::error::Error for BehaviorTreeError {}

impl<E: serde::de::Error> From<E> for BehaviorTreeError {
  fn from(e: E) -> Self {
    BehaviorTreeError::ParsingError {
      message: e.to_string(),
    }
  }
}

impl Into<CallError> for BehaviorTreeError {
  fn into(self) -> CallError {
    match self {
      BehaviorTreeError::CallError(e) => e,
      _ => CallError::Generic {
        message: self.to_string(),
      },
    }
  }
}
