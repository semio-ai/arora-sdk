use arora_types::{module::high::TypeRef, value::Value};
use serde::{Deserialize, Serialize};
use std::{cell::RefCell, collections::HashMap, fmt::Display, rc::Rc};
use uuid::Uuid;

/// Special parameter UUID used to capture a node's return value into a variable.
/// When present in a node's `arguments`, the function's return value is written
/// to the bound variable and the node always reports Success.
/// UUID: 5f726574-0000-4000-8000-000000000000 ("_ret" in ASCII)
pub const _RET_PARAM_ID: Uuid = Uuid::from_bytes([
  0x5f, 0x72, 0x65, 0x74, 0x00, 0x00, 0x40, 0x00, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
]);

#[derive(Serialize, Deserialize, Debug)]
pub struct Variable {
  name: String,
  ty: TypeRef,
}

#[derive(Serialize, Deserialize, Debug, Default, PartialEq)]
pub struct Node {
  /// The ID of this node.
  pub id: Uuid,

  /// The ID of the function in the registry to call on ticks.
  pub function: Uuid,

  /// Args to apply to the function call parameters.
  #[serde(default)]
  pub arguments: HashMap<Uuid, Expression>,

  /// Child nodes, if any.
  #[serde(default)]
  pub children: Option<Vec<Uuid>>,
}

impl Display for Node {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.write_fmt(format_args!("node {}", self.id))
  }
}

/// An expression describing a variable in the context of the behavior tree.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(rename_all = "snake_case")]
pub enum Expression {
  /// Generic value description.
  Value(Value),
  /// Specific UUID value.
  Uuid(Uuid),
  /// Reference to an anonymous variable.
  #[serde(skip)]
  Variable(Rc<RefCell<Value>>),
  /// Reference to a variable by ID.
  VariableId(Uuid),
  /// Reference to another parameter.
  NodeArgument(NodeParameterId),
  /// Local variable resulting from some computation.
  Call(Box<CallExpression>),
}

/// An expression describing a function to call.
/// Every piece of it can be expressed dynamically.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct CallExpression {
  /// The module ID.
  /// The result of the expression must be an UUID.
  pub module: Expression,
  /// The function ID.
  /// The result of the expression must be an UUID.
  pub function: Expression,
  /// The arguments.
  /// The result of the key expressions must be UUIDs of parameters.
  /// The result of the value expressions can be any value.
  pub arguments: Vec<(Expression, Expression)>,
}

/// An identifier for the parameter of a node in the context of a behavior tree.
#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq, Hash)]
pub struct NodeParameterId {
  pub node: Uuid,
  pub parameter: Uuid,
}

impl Display for NodeParameterId {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.write_fmt(format_args!(
      "node parameter {}.{}",
      self.node, self.parameter
    ))
  }
}

#[cfg(test)]
pub mod tests {
  use super::*;
  use anyhow::Result;
  use std::str::FromStr;

  pub const TRIVIAL_NODE_YAML: &str = "\
id: fc8e2c43-8f0a-461f-9b44-30cc45c4357f
function: af2bd9fa-14f6-4388-b68b-e50c8443960e
";

  #[test]
  pub fn parse_trivial_node() -> Result<()> {
    let node_yaml = TRIVIAL_NODE_YAML;
    let expected = Node {
      id: Uuid::from_str("fc8e2c43-8f0a-461f-9b44-30cc45c4357f")?,
      function: Uuid::from_str("af2bd9fa-14f6-4388-b68b-e50c8443960e")?,
      ..Default::default()
    };
    let actual: Node = serde_yaml::from_str(node_yaml)?;
    assert!(actual == expected);
    Ok(())
  }

  pub const SIMPLE_TREE_YAML: &str = "\
- id: fc8e2c43-8f0a-461f-9b44-30cc45c4357f
  function: af2bd9fa-14f6-4388-b68b-e50c8443960e
  children:
    - d50638bf-c44b-4f6e-a5f2-925fcfff71a8
    - 817e45e3-26ca-45a4-8537-ad70e3de1298
- id: d50638bf-c44b-4f6e-a5f2-925fcfff71a8
  function: 418e7f79-9df8-4fe4-92f9-54f9fc6e2de8
  arguments:
    85710898-406b-464d-bf9c-21ac658dbc04:
      variable_id:
        d775359e-9f6b-4c1e-892c-8a4a36ec82d0
- id: 817e45e3-26ca-45a4-8537-ad70e3de1298
  function: 77c7bfa6-c01f-416b-a09f-5d2a8e63d4e0
";

  #[test]
  pub fn parse_simple_tree() -> Result<()> {
    let node_yaml = SIMPLE_TREE_YAML;
    let expected = vec![
      Node {
        id: Uuid::from_str("fc8e2c43-8f0a-461f-9b44-30cc45c4357f")?,
        function: Uuid::from_str("af2bd9fa-14f6-4388-b68b-e50c8443960e")?,
        children: Some(vec![
          Uuid::from_str("d50638bf-c44b-4f6e-a5f2-925fcfff71a8")?,
          Uuid::from_str("817e45e3-26ca-45a4-8537-ad70e3de1298")?,
        ]),
        ..Default::default()
      },
      Node {
        id: Uuid::from_str("d50638bf-c44b-4f6e-a5f2-925fcfff71a8")?,
        function: Uuid::from_str("418e7f79-9df8-4fe4-92f9-54f9fc6e2de8")?,
        arguments: HashMap::from([(
          Uuid::from_str("85710898-406b-464d-bf9c-21ac658dbc04")?,
          Expression::VariableId(Uuid::from_str("d775359e-9f6b-4c1e-892c-8a4a36ec82d0")?),
        )]),
        ..Default::default()
      },
      Node {
        id: Uuid::from_str("817e45e3-26ca-45a4-8537-ad70e3de1298")?,
        function: Uuid::from_str("77c7bfa6-c01f-416b-a09f-5d2a8e63d4e0")?,
        ..Default::default()
      },
    ];

    let actual: Vec<Node> = serde_yaml::from_str(node_yaml)?;
    assert!(actual == expected);
    Ok(())
  }
}
