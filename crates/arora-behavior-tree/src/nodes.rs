use std::{cell::RefCell, collections::HashMap, rc::Rc};

use arora_schema::value::Value;
use uuid::Uuid;

use crate::{
  error::BehaviorTreeError,
  schema::{Expression, Node, NodeParameterId},
  setup_node_parameter_variable, BehaviorTree,
};

#[allow(unused)]
pub fn succeed() -> TreeNode {
  TreeNode::action_node(SUCCEED_FUNCTION_ID.clone())
}

#[allow(unused)]
pub fn fail() -> TreeNode {
  TreeNode::action_node(FAIL_FUNCTION_ID.clone())
}

#[allow(unused)]
pub fn run() -> TreeNode {
  TreeNode::action_node(RUN_FUNCTION_ID.clone())
}

#[allow(unused)]
pub fn status_identity(value: Rc<RefCell<Value>>) -> TreeNode {
  TreeNode {
    function: STATUS_IDENTITY_FUNCTION_ID.clone(),
    children: None,
    parameters: HashMap::from([(STATUS_VALUE_PARAM_ID.clone(), Expression::Variable(value))]),
  }
}

#[allow(unused)]
pub fn seq(children: Vec<TreeNode>) -> TreeNode {
  TreeNode::control_node(SEQ_FUNCTION_ID.clone(), children)
}

#[allow(unused)]
pub fn seq_star(children: Vec<TreeNode>) -> TreeNode {
  TreeNode {
    function: SEQ_STAR_FUNCTION_ID.clone(),
    children: Some(children),
    parameters: HashMap::from([(
      SEQ_STAR_CURRENT_INDEX_PARAM_ID.clone(),
      Expression::Value(Value::U16(0)),
    )]),
  }
}

#[allow(unused)]
pub fn fallback(children: Vec<TreeNode>) -> TreeNode {
  TreeNode::control_node(FALLBACK_FUNCTION_ID.clone(), children)
}

#[allow(unused)]
pub fn parallel(children: Vec<TreeNode>) -> TreeNode {
  TreeNode::control_node(PARALLEL_FUNCTION_ID.clone(), children)
}

pub struct TreeNode {
  pub function: Uuid,
  pub children: Option<Vec<TreeNode>>,
  pub parameters: HashMap<Uuid, Expression>,
}

/// Represents a tree of node with direct relations to children,
/// instead of using UUID references.
#[allow(unused)]
impl TreeNode {
  /// Helper to construct an action node.
  pub fn action_node(function: Uuid) -> Self {
    Self {
      function,
      children: None,
      parameters: HashMap::new(),
    }
  }

  /// Helper to construct a control node.
  pub fn control_node(function: Uuid, children: Vec<TreeNode>) -> Self {
    Self {
      function,
      children: Some(children),
      parameters: HashMap::new(),
    }
  }

  /// Moves the data to this node into components of a behavior tree, recursively.
  pub fn collect(
    self,
    mut node_index: &mut HashMap<Uuid, Rc<Node>>,
    mut variables: &mut HashMap<Uuid, Rc<RefCell<Value>>>,
    mut node_parameters_variables: &mut HashMap<NodeParameterId, Rc<RefCell<Value>>>,
  ) -> Result<Rc<Node>, BehaviorTreeError> {
    let node_id = Uuid::new_v4();
    let children: Option<Vec<Uuid>> = if let Some(children) = self.children {
      let mut ids = Vec::with_capacity(children.len());
      for child in children {
        let child_node =
          child.collect(&mut node_index, &mut variables, node_parameters_variables)?;
        let child_node_id = child_node.id.clone();
        // This could only happen with an UUID collision, i.e. never.
        assert_eq!(node_index.insert(child_node.id.clone(), child_node), None);
        ids.push(child_node_id);
      }
      Some(ids)
    } else {
      None
    };
    let mut arguments = HashMap::new();
    for (param_id, expression) in self.parameters {
      let node_parameter = NodeParameterId {
        node: node_id.to_owned(),
        parameter: param_id.to_owned(),
      };
      setup_node_parameter_variable(
        &node_parameter,
        &expression,
        variables,
        node_parameters_variables,
      )?;
      arguments.insert(param_id, expression);
    }
    Ok(Rc::new(Node {
      id: node_id,
      function: self.function,
      arguments,
      children,
    }))
  }
}

/// Transforms the tree of nodes into a behavior tree that can be run.
impl TryInto<BehaviorTree> for TreeNode {
  type Error = BehaviorTreeError;
  fn try_into(self) -> Result<BehaviorTree, Self::Error> {
    let mut node_index = HashMap::new();
    let mut variables = HashMap::new();
    let mut node_arg_variables = HashMap::new();
    let root = self.collect(&mut node_index, &mut variables, &mut node_arg_variables)?;
    Ok(BehaviorTree {
      root,
      node_index,
      variables: Rc::new(RefCell::new(variables)),
      node_arg_variables: Rc::new(node_arg_variables),
    })
  }
}

lazy_static::lazy_static! {
  static ref SUCCEED_FUNCTION_ID: Uuid = Uuid::parse_str("6696F0BD-E781-40CD-AEB5-8DC616F810D2").unwrap();
  static ref FAIL_FUNCTION_ID: Uuid = Uuid::parse_str("3abbbfb6-d00d-41eb-88bb-97874267eaf6").unwrap();
  static ref RUN_FUNCTION_ID: Uuid = Uuid::parse_str("41ae5ed0-1d12-4b71-aab8-02e7efedf177").unwrap();
  static ref STATUS_IDENTITY_FUNCTION_ID: Uuid = Uuid::parse_str("ef48e6d3-c735-4b5c-8f63-fc54d94dd4ee").unwrap();
  static ref STATUS_VALUE_PARAM_ID: Uuid = Uuid::parse_str("e1f174e6-ca9e-4344-84cb-7f3f22115239").unwrap();
  static ref SEQ_FUNCTION_ID: Uuid = Uuid::parse_str("32246df6-ab5d-4f18-9221-23e28731de93").unwrap();
  static ref SEQ_STAR_FUNCTION_ID: Uuid = Uuid::parse_str("c2d5ed72-798c-4174-94f7-13378bd9bf1f").unwrap();
  static ref SEQ_STAR_CURRENT_INDEX_PARAM_ID: Uuid = Uuid::parse_str("4de502df-3f48-4541-94d8-dd68fe92bc8e").unwrap();
  static ref FALLBACK_FUNCTION_ID: Uuid = Uuid::parse_str("bfa89a4e-c369-430e-be78-0dc07311391c").unwrap();
  static ref PARALLEL_FUNCTION_ID: Uuid = Uuid::parse_str("a9340289-1f30-411f-9faa-0f07d54613e8").unwrap();
}
