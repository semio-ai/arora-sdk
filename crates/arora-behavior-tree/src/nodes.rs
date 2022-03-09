use std::{collections::HashMap, rc::Rc, cell::RefCell};

use arora_schema::value::Value;
use uuid::Uuid;

use crate::{schema::Node, BehaviorTree};

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
    parameters: HashMap::from([(STATUS_VALUE_PARAM_ID.clone(), value)]),
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
    parameters: HashMap::from([(SEQ_STAR_CURRENT_INDEX_PARAM_ID.clone(), Rc::new(RefCell::new(Value::U16(0))))]),
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
  pub parameters: HashMap<Uuid, Rc<RefCell<Value>>>,
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
    mut locals: &mut HashMap<Uuid, Rc<RefCell<Value>>>,
  ) -> Rc<Node> {
    let children: Option<Vec<Uuid>> = self.children.map(|children| {
      let mut ids = Vec::with_capacity(children.len());
      for child in children {
        let node = child.collect(&mut node_index, &mut locals);
        let node_id = node.id.clone();
        // This could only happen with an UUID collision, i.e. never.
        assert_eq!(node_index.insert(node.id.clone(), node), None);
        ids.push(node_id);
      }
      ids
    });
    let mut arguments = HashMap::new();
    for (param_id, value) in self.parameters {
      let value_id = Uuid::new_v4();
      locals.insert(value_id.clone(), value);
      arguments.insert(param_id, value_id);
    }
    Rc::new(Node {
      id: Uuid::new_v4(),
      function: self.function,
      arguments,
      children,
    })
  }
}

/// Transforms the tree of nodes into a behavior tree that can be run.
impl Into<BehaviorTree> for TreeNode {
  fn into(self) -> BehaviorTree {
    let mut node_index = HashMap::new();
    let mut locals = HashMap::new();
    let root = self.collect(&mut node_index, &mut locals);
    BehaviorTree {
      root,
      node_index,
      locals: Rc::new(RefCell::new(locals)),
    }
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
