mod error;
mod nodes;
mod schema;
mod status;
mod tests;
mod tick_id;
use arora::call::{Call, CallBridge, CallError, Callable, CallableId};
use arora_index::Index;
use arora_schema::{
  module::low::{Parameter, TypeRef},
  value::{ConversionError, StructureField, Value},
};
use error::BehaviorTreeError;
use schema::Node;
use status::Status;
use std::{cell::RefCell, collections::HashMap, rc::Rc};
use tick_id::TickId;
use uuid::Uuid;

use crate::tick_id::{TICK_ID_ID_FIELD_ID, TICK_ID_TYPE_ID};

// Runtime.
//====================================================================
/// The behavior tree, binding all nodes, variables and types together.
pub struct BehaviorTree {
  /// The root node from which the tree stems.
  root: Rc<Node>,
  /// All the nodes, indexed by their ID.
  node_index: HashMap<Uuid, Rc<Node>>,
  /// The local variables.
  locals: Rc<RefCell<HashMap<Uuid, Rc<RefCell<Value>>>>>,
}

struct BehaviorTreeRuntime<'a> {
  caller: &'a mut dyn CallBridge,
  tick: TickId,
}

impl<'a> BehaviorTreeRuntime<'a> {
  fn setup(
    tree: &'a BehaviorTree,
    index: Rc<Index>,
    caller: &'a mut dyn CallBridge,
  ) -> Result<Self, BehaviorTreeError> {
    let tick = setup_tick_function(
      tree.root.clone(),
      &tree.node_index,
      index.clone(),
      tree.locals.clone(),
      caller,
    )?;
    Ok(Self { caller, tick })
  }

  fn tick(&mut self) -> Result<Status, BehaviorTreeError> {
    self.tick.tick(self.caller)
  }
}

/// Runs a behavior tree until it reaches the status success or failure.
pub fn run_behavior_tree(
  behavior: &BehaviorTree,
  index: Rc<Index>,
  caller: &mut dyn CallBridge,
) -> Result<status::Status, BehaviorTreeError> {
  let mut runtime = BehaviorTreeRuntime::setup(behavior, index, caller)?;
  let mut status = Status::Running;
  while status == Status::Running {
    status = runtime.tick()?;
  }
  return Ok(status);
}

fn setup_tick_function(
  node: Rc<Node>,
  node_index: &HashMap<Uuid, Rc<Node>>,
  index: Rc<Index>,
  locals: Rc<RefCell<HashMap<Uuid, Rc<RefCell<Value>>>>>,
  caller: &mut dyn CallBridge,
) -> Result<TickId, BehaviorTreeError> {
  let nof_children = node
    .children
    .as_ref()
    .map(|children| children.len())
    .unwrap_or(0);
  let mut children_ticks: Vec<TickId> = Vec::with_capacity(nof_children);
  if let Some(children) = &node.children {
    for child_id in children {
      let child_node = node_index
        .get(child_id)
        .ok_or(BehaviorTreeError::ChildNodeNotFound {
          node: node.id.clone(),
          child: child_id.clone(),
        })?
        .clone();
      let tick_function_with_id = setup_tick_function(
        child_node.clone(),
        &node_index,
        index.clone(),
        locals.clone(),
        caller,
      )?;
      children_ticks.push(tick_function_with_id);
    }
  }
  let tick_function: Rc<dyn Callable> = Rc::new(TickFunction {
    node: node.clone(),
    index,
    locals: locals.to_owned(),
    children: children_ticks,
  });
  let callable_id = caller.arora_register_callable(tick_function);
  Ok(callable_id.into())
}

fn tick(
  caller: &mut dyn CallBridge,
  index: Rc<Index>,
  locals: Rc<RefCell<HashMap<Uuid, Rc<RefCell<Value>>>>>,
  child_tick_ids: &Vec<TickId>,
  node: Rc<Node>,
) -> Result<status::Status, BehaviorTreeError> {
  let function = index.find_function(&node.function).map_err(|_| {
    BehaviorTreeError::CallError(CallError::FunctionNotFound {
      id: node.function.clone(),
    })
  })?;

  let mut call = Call {
    id: node.function.clone(),
    args: Vec::with_capacity(node.arguments.len() + if node.children.is_some() { 1 } else { 0 }),
  };

  let nof_children = node
    .children
    .as_ref()
    .map(|children| children.len())
    .unwrap_or(0);
  assert_eq!(nof_children, child_tick_ids.len());

  if let Some(_) = &node.children {
    // Find the `children` parameter by its type
    let children_params: Vec<&Parameter> = function
      .parameters
      .iter()
      .filter(|parameter| {
        if let TypeRef::Array { id: param_id } = parameter.ty {
          param_id == *TICK_ID_TYPE_ID && parameter.name == "children"
        } else {
          false
        }
      })
      .collect();

    let children_param: &Parameter = if children_params.is_empty() {
      Err(BehaviorTreeError::MissingChildrenParameter {
        node: node.id.clone(),
      })
    } else if children_params.len() > 1 {
      Err(BehaviorTreeError::InternalError {
        message: "two args are named \"children\" and accept an array of TickId".to_string(),
      })
    } else {
      children_params
        .first()
        .ok_or(BehaviorTreeError::InternalError {
          message: "single child parameter cannot be accessed".to_string(),
        })
    }?;

    // Pass the tick ids of the children.
    let mut children_arg = Vec::with_capacity(child_tick_ids.len());
    for child_tick_id in child_tick_ids {
      children_arg.push(arora_schema::value::StructureWithoutId {
        fields: vec![StructureField {
          id: *TICK_ID_ID_FIELD_ID,
          value: Box::new(Value::U64(child_tick_id.callable_id)),
        }],
      });
    }
    call.args.push(StructureField {
      id: children_param.id.clone(),
      value: Box::new(Value::ArrayStructure {
        id: *TICK_ID_TYPE_ID,
        elements: children_arg,
      }),
    })
  }

  // Pass the remaining parameters from the behavior-wise variables.
  {
    let locals = locals.borrow();
    for (param_id, variable_id) in &node.arguments {
      let value = locals
        .get(variable_id)
        .ok_or(BehaviorTreeError::VariableNotFound {
          variable: variable_id.clone(),
          node: node.id.clone(),
        })?;
      call.args.push(StructureField {
        id: param_id.clone(),
        value: Box::new(value.borrow().clone()),
      });
    }
  }

  let result = caller
    .arora_call(&function.module, call)
    .map_err(|e| BehaviorTreeError::CallError(e))?;

  let mut mutable_locals = variables.borrow_mut();
  for mutated in result.mutated {
    let variable = locals
      .get_mut(&mutated.id)
      .ok_or(BehaviorTreeError::InternalError {
        message: format!(
          "mutated parameter {} does not correspond to any local variable",
          &mutated.id
        ),
      })?;
    *variable.borrow_mut() = *mutated.value;
  }

  result
    .ret
    .try_into()
    .map_err(|e| BehaviorTreeError::ConversionError(e))
}

/// Specialization of Callable that returns a Status.
trait Tickable {
  fn tick(&self, caller: &mut dyn CallBridge) -> Result<status::Status, BehaviorTreeError>;
}

impl Tickable for TickId {
  fn tick(&self, caller: &mut dyn CallBridge) -> Result<status::Status, BehaviorTreeError> {
    CallableId::from(self).tick(caller)
  }
}

impl Tickable for CallableId {
  fn tick(&self, caller: &mut dyn CallBridge) -> Result<status::Status, BehaviorTreeError> {
    let value = self
      .call(caller)
      .map_err(|e| BehaviorTreeError::CallError(e))?;
    value.try_into().map_err(|_| {
      BehaviorTreeError::ConversionError(ConversionError {
        message: "return value cannot be interpreted as a Status".to_string(),
      })
    })
  }
}

/// The usual Tickable object in behavior trees, which is also Callable.
struct TickFunction {
  node: Rc<Node>,
  index: Rc<Index>,
  locals: Rc<RefCell<HashMap<Uuid, Rc<RefCell<Value>>>>>,
  children: Vec<TickId>,
}

impl Tickable for TickFunction {
  fn tick(&self, caller: &mut dyn CallBridge) -> Result<status::Status, BehaviorTreeError> {
    tick(
      caller,
      self.index.clone(),
      self.locals.clone(),
      &self.children,
      self.node.clone(),
    )
  }
}

impl Callable for TickFunction {
  fn call(&self, caller: &mut dyn CallBridge) -> Result<Value, CallError> {
    self
      .tick(caller)
      .map(Into::<Value>::into)
      .map_err(Into::<CallError>::into)
  }
}

// Loading behavior trees.
//====================================================================
pub fn load_behavior_tree_nodes(nodes: Vec<Node>) -> Result<BehaviorTree, BehaviorTreeError> {
  let mut node_index: HashMap<Uuid, Rc<Node>> = HashMap::new();
  let mut root: Option<Rc<Node>> = None;
  for node in nodes {
    let shared_node = Rc::new(node);
    if root.is_none() {
      // first node is the root?
      root = Some(shared_node.clone());
    }
    let existing_node = node_index.insert(shared_node.id.clone(), shared_node.clone());
    if let Some(existing_node) = existing_node {
      return Err(BehaviorTreeError::InconsistentTreeError {
        message: format!("duplicate node {}", existing_node.id),
      });
    }
  }

  Ok(BehaviorTree {
    root: root.unwrap(),
    node_index,
    locals: Rc::new(RefCell::new(HashMap::new())),
  })
}
