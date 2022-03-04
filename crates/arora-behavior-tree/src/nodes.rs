use std::{collections::HashMap, rc::Rc};

use uuid::Uuid;

use crate::{schema::Node, BehaviorTree};

pub fn succeed() -> TreeNode {
  TreeNode::action_node(SUCCEED_FUNCTION_ID.clone())
}

pub fn fail() -> TreeNode {
  TreeNode::action_node(FAIL_FUNCTION_ID.clone())
}

pub fn run() -> TreeNode {
  TreeNode::action_node(RUN_FUNCTION_ID.clone())
}

pub fn seq(children: Vec<TreeNode>) -> TreeNode {
  TreeNode::control_node(SEQ_FUNCTION_ID.clone(), children)
}

pub struct TreeNode {
  pub function: Uuid,
  pub children: Option<Vec<TreeNode>>,
}

/// Represents a tree of node with direct relations to children,
/// instead of using UUID references.
impl TreeNode {
  /// Helper to construct an action node.
  pub fn action_node(function: Uuid) -> Self {
    Self {
      function,
      children: None,
    }
  }

  /// Helper to construct a control node.
  pub fn control_node(function: Uuid, children: Vec<TreeNode>) -> Self {
    Self {
      function,
      children: Some(children),
    }
  }

  /// Moves the data to this node into components of a behavior tree, recursively.
  pub fn collect(self, mut node_index: &mut HashMap<Uuid, Rc<Node>>) -> Rc<Node> {
    let children: Option<Vec<Uuid>> = self.children.map(|children| {
      let mut ids = Vec::with_capacity(children.len());
      for child in children {
        let node = child.collect(&mut node_index);
        let node_id = node.id.clone();
        // This could only happen with an UUID collision, i.e. never.
        assert_eq!(node_index.insert(node.id.clone(), node), None);
        ids.push(node_id);
      }
      ids
    });
    Rc::new(Node {
      id: Uuid::new_v4(),
      function: self.function,
      arguments: HashMap::new(),
      children,
    })
  }
}

/// Transforms the tree of nodes into a behavior tree that can be run.
impl Into<BehaviorTree> for TreeNode {
  fn into(self) -> BehaviorTree {
    let mut node_index = HashMap::new();
    let root = self.collect(&mut node_index);
    BehaviorTree { root, node_index }
  }
}

lazy_static::lazy_static! {
  static ref SUCCEED_FUNCTION_ID: Uuid = Uuid::parse_str("6696F0BD-E781-40CD-AEB5-8DC616F810D2").unwrap();
  static ref FAIL_FUNCTION_ID: Uuid = Uuid::parse_str("3abbbfb6-d00d-41eb-88bb-97874267eaf6").unwrap();
  static ref RUN_FUNCTION_ID: Uuid = Uuid::parse_str("41ae5ed0-1d12-4b71-aab8-02e7efedf177").unwrap();
  static ref SEQ_FUNCTION_ID: Uuid = Uuid::parse_str("32246df6-ab5d-4f18-9221-23e28731de93").unwrap();
}
