use crate::{schema::Expression, tree_node::TreeNode};
use arora_schema::value::Value;
use std::{cell::RefCell, collections::HashMap, rc::Rc};
use uuid::Uuid;

// To simulate statuses
//===============================================================
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

// Basic data-oriented action nodes
//==============================================================
#[allow(unused)]
pub fn store(storage: Expression, value: Expression) -> TreeNode {
  TreeNode {
    function: STORE_FUNCTION_ID.clone(),
    children: None,
    parameters: HashMap::from([
      (STORE_STORAGE_PARAM_ID.clone(), storage),
      (STORE_VALUE_PARAM_ID.clone(), value),
    ]),
  }
}

#[allow(unused)]
pub fn increase(storage: Expression, delta: Expression) -> TreeNode {
  TreeNode {
    function: INCREASE_FUNCTION_ID.clone(),
    children: None,
    parameters: HashMap::from([
      (INCREASE_STORAGE_PARAM_ID.clone(), storage),
      (INCREASE_DELTA_PARAM_ID.clone(), delta),
    ]),
  }
}

// Basic control nodes
//==============================================================
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

// Other functions from other modules
//================================================================
#[allow(unused)]
pub fn cos(angle: Expression, res: Expression) -> TreeNode {
  TreeNode {
    function: COS_FUNCTION_ID.clone(),
    children: None,
    parameters: HashMap::from([
      (COS_ANGLE_PARAM_ID.clone(), angle),
      (COS_RES_PARAM_ID.clone(), res),
    ]),
  }
}

lazy_static::lazy_static! {
  static ref SUCCEED_FUNCTION_ID: Uuid = Uuid::parse_str("6696F0BD-E781-40CD-AEB5-8DC616F810D2").unwrap();
  static ref FAIL_FUNCTION_ID: Uuid = Uuid::parse_str("3abbbfb6-d00d-41eb-88bb-97874267eaf6").unwrap();
  static ref RUN_FUNCTION_ID: Uuid = Uuid::parse_str("41ae5ed0-1d12-4b71-aab8-02e7efedf177").unwrap();
  static ref STATUS_IDENTITY_FUNCTION_ID: Uuid = Uuid::parse_str("ef48e6d3-c735-4b5c-8f63-fc54d94dd4ee").unwrap();
  static ref STATUS_VALUE_PARAM_ID: Uuid = Uuid::parse_str("e1f174e6-ca9e-4344-84cb-7f3f22115239").unwrap();

  static ref STORE_FUNCTION_ID: Uuid = Uuid::parse_str("b8349b96-abc7-4a31-906c-da1ce6fa356e").unwrap();
  static ref STORE_STORAGE_PARAM_ID: Uuid = Uuid::parse_str("2345a3a5-a80d-4480-9927-3c65bd2b7543").unwrap();
  static ref STORE_VALUE_PARAM_ID: Uuid = Uuid::parse_str("0a0778cd-cb7a-41fc-96d4-512cc8538ce2").unwrap();

  static ref INCREASE_FUNCTION_ID: Uuid = Uuid::parse_str("7f6fc4a9-567c-4f15-87cc-7ca34ae1456f").unwrap();
  static ref INCREASE_STORAGE_PARAM_ID: Uuid = Uuid::parse_str("e898fe88-cc61-46d2-aecc-b4fc0beb862f").unwrap();
  static ref INCREASE_DELTA_PARAM_ID: Uuid = Uuid::parse_str("1018eb85-2d04-4995-a349-b6c83c27f287").unwrap();

  static ref SEQ_FUNCTION_ID: Uuid = Uuid::parse_str("32246df6-ab5d-4f18-9221-23e28731de93").unwrap();
  static ref SEQ_STAR_FUNCTION_ID: Uuid = Uuid::parse_str("c2d5ed72-798c-4174-94f7-13378bd9bf1f").unwrap();
  static ref SEQ_STAR_CURRENT_INDEX_PARAM_ID: Uuid = Uuid::parse_str("4de502df-3f48-4541-94d8-dd68fe92bc8e").unwrap();
  static ref FALLBACK_FUNCTION_ID: Uuid = Uuid::parse_str("bfa89a4e-c369-430e-be78-0dc07311391c").unwrap();
  static ref PARALLEL_FUNCTION_ID: Uuid = Uuid::parse_str("a9340289-1f30-411f-9faa-0f07d54613e8").unwrap();

  static ref COS_FUNCTION_ID: Uuid = Uuid::parse_str("104b9710-5d43-4a93-944c-d64bddb30ef8").unwrap();
  static ref COS_ANGLE_PARAM_ID: Uuid = Uuid::parse_str("272fbafd-c2a5-4ffe-a294-9cabe6e6c1e7").unwrap();
  static ref COS_RES_PARAM_ID: Uuid = Uuid::parse_str("1d101686-05d8-47b4-9292-fdc9e5a0daeb").unwrap();
}
