use crate::{
  error::BehaviorTreeError,
  schema::{Expression, Node, NodeParameterId},
  setup_node_parameter_variable, BehaviorTree,
};
use arora_types::value::Value;
use std::{cell::RefCell, collections::HashMap, rc::Rc};
use uuid::Uuid;

// Helpers to make trees
//================================================================
pub struct TreeNode {
  pub function: Uuid,
  pub children: Option<Vec<TreeNode>>,
  pub parameters: HashMap<Uuid, Expression>,
  /// If set, the function's return value is written to this variable on each tick.
  pub return_binding: Option<Rc<RefCell<Value>>>,
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
      return_binding: None,
    }
  }

  /// Helper to construct a control node.
  pub fn control_node(function: Uuid, children: Vec<TreeNode>) -> Self {
    Self {
      function,
      children: Some(children),
      parameters: HashMap::new(),
      return_binding: None,
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
      return_binding: self.return_binding.map(Expression::Variable),
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
