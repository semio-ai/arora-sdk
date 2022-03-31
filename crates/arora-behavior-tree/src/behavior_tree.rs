mod error;
mod nodes;
mod schema;
mod schema_groot;
mod status;
mod tests;
mod tick_id;
mod tree_node;
use arora::call::{Call, CallBridge, CallError, Callable, CallableId};
use arora_index::Index;
use arora_schema::{
  module::low::{Parameter, TypeRef},
  value::{ConversionError, StructureField, Value},
};
use error::BehaviorTreeError;
use schema::{CallExpression, Node, NodeParameterId};
use status::Status;
use std::{cell::RefCell, collections::HashMap, rc::Rc};
use tick_id::TickId;
use uuid::Uuid;

use crate::{
  schema::Expression,
  tick_id::{TICK_ID_ID_FIELD_ID, TICK_ID_TYPE_ID},
};

// Runtime.
//====================================================================
/// The behavior tree, binding all nodes, variables and types together.
pub struct BehaviorTree {
  /// The root node from which the tree stems.
  root: Rc<Node>,
  /// All the nodes, indexed by their ID.
  node_index: HashMap<Uuid, Rc<Node>>,
  /// The local variables.
  variables: Rc<RefCell<HashMap<Uuid, Rc<RefCell<Value>>>>>,
  /// Variables associated to node arguments (node, arg).
  node_arg_variables: Rc<HashMap<NodeParameterId, Rc<RefCell<Value>>>>,
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
      tree.variables.clone(),
      tree.node_arg_variables.clone(),
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
  variables: Rc<RefCell<HashMap<Uuid, Rc<RefCell<Value>>>>>,
  node_arg_variables: Rc<HashMap<NodeParameterId, Rc<RefCell<Value>>>>,
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
        variables.clone(),
        node_arg_variables.clone(),
        caller,
      )?;
      children_ticks.push(tick_function_with_id);
    }
  }
  let tick_function: Rc<dyn Callable> = Rc::new(TickFunction {
    node: node.clone(),
    index,
    locals: variables.to_owned(),
    node_arg_variables: node_arg_variables.to_owned(),
    children: children_ticks,
  });
  let callable_id = caller.arora_register_callable(tick_function);
  Ok(callable_id.into())
}

fn tick(
  caller: &mut dyn CallBridge,
  index: Rc<Index>,
  variables: Rc<RefCell<HashMap<Uuid, Rc<RefCell<Value>>>>>,
  node_parameters_variables: Rc<HashMap<NodeParameterId, Rc<RefCell<Value>>>>,
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

  // Resolving the remaining parameters, and passing them.
  // A local map of variables is maintained to update them when if mutated.
  // Some of them were setup to be shared, but it is transparent here.
  // They are indexed by parameter id.
  let mut locals = HashMap::<Uuid, Rc<RefCell<Value>>>::new();
  {
    let variables = variables.borrow();
    for (param_id, value_expression) in &node.arguments {
      let node_parameter = NodeParameterId {
        node: node.id.to_owned(),
        parameter: param_id.to_owned(),
      };
      let variable = get_node_parameter_variable(&node_parameter, &node_parameters_variables)?;

      // If the parameter expression is a call, perform the call to update the value.
      match value_expression {
        Expression::Call(parameter_call_expression) => {
          let value = call_expression(
            &variables,
            &node_parameters_variables,
            &parameter_call_expression,
            caller,
            &NodeParameterId {
              node: node.id,
              parameter: param_id.to_owned(),
            },
          )?;
          *variable.borrow_mut() = value;
        }
        _ => {}
      };

      locals.insert(param_id.clone(), variable.clone());
      call.args.push(StructureField {
        id: param_id.clone(),
        value: Box::new(variable.borrow().clone()),
      });
    }
  }

  let result = caller
    .arora_call(&function.module, call)
    .map_err(|e| BehaviorTreeError::CallError(e))?;

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
  node_arg_variables: Rc<HashMap<NodeParameterId, Rc<RefCell<Value>>>>,
  children: Vec<TickId>,
}

impl Tickable for TickFunction {
  fn tick(&self, caller: &mut dyn CallBridge) -> Result<status::Status, BehaviorTreeError> {
    tick(
      caller,
      self.index.clone(),
      self.locals.clone(),
      self.node_arg_variables.clone(),
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
  let mut variables = HashMap::new();
  let mut node_parameters_variables = HashMap::new();
  for node in nodes {
    let shared_node = Rc::new(node);
    if root.is_none() {
      // first node is the root?
      root = Some(shared_node.clone());
    }

    // Index the node and check for duplicates.
    let existing_node = node_index.insert(shared_node.id.clone(), shared_node.clone());
    if let Some(existing_node) = existing_node {
      return Err(BehaviorTreeError::InconsistentTreeError {
        message: format!("duplicate node {}", existing_node.id),
      });
    }

    // Setup variables for every parameter.
    for (param_id, arg_expr) in &shared_node.arguments {
      let node_param = NodeParameterId {
        node: shared_node.id.to_owned(),
        parameter: param_id.to_owned(),
      };
      setup_node_parameter_variable(
        &node_param,
        &arg_expr,
        &mut variables,
        &mut node_parameters_variables,
      )?;
    }
  }

  Ok(BehaviorTree {
    root: root.unwrap(),
    node_index,
    variables: Rc::new(RefCell::new(variables)),
    node_arg_variables: Rc::new(node_parameters_variables),
  })
}

// Other helpers
//=======================================================
fn get_variable<'a>(
  variables: &'a HashMap<Uuid, Rc<RefCell<Value>>>,
  variable_id: &Uuid,
  node_id: &Uuid,
) -> Result<&'a Rc<RefCell<Value>>, BehaviorTreeError> {
  variables
    .get(variable_id)
    .ok_or(BehaviorTreeError::VariableNotFound {
      variable: variable_id.to_owned(),
      node: node_id.to_owned(),
    })
}

fn get_node_parameter_variable<'a>(
  node_parameter: &NodeParameterId,
  node_parameters_variables: &'a HashMap<NodeParameterId, Rc<RefCell<Value>>>,
) -> Result<&'a Rc<RefCell<Value>>, BehaviorTreeError> {
  node_parameters_variables
    .get(&node_parameter)
    .ok_or(BehaviorTreeError::InconsistentTreeError {
      message: format!("node parameter {} was not found", node_parameter),
    })
}

/// Sets up a variable for the given node parameter.
/// A variable will always be added to the `node_parameters_variables`.
/// If the parameter refers to a variable that does not exist yet,
/// it will be created with the default value `Value::Unit`.
/// If a parameter has to be computed with a function call,
/// the variable will hold the default value `Value::Unit`.
fn setup_node_parameter_variable(
  node_parameter: &NodeParameterId,
  argument_expression: &Expression,
  variables: &mut HashMap<Uuid, Rc<RefCell<Value>>>,
  node_parameters_variables: &mut HashMap<NodeParameterId, Rc<RefCell<Value>>>,
) -> Result<Rc<RefCell<Value>>, BehaviorTreeError> {
  let variable = match argument_expression {
    Expression::Value(value) => Rc::new(RefCell::new(value.to_owned())),
    Expression::Uuid(uuid) => {
      let value = Value::ArrayU8(uuid.as_bytes().to_vec());
      Rc::new(RefCell::new(value))
    }
    Expression::Variable(variable) => variable.clone(),
    Expression::VariableId(variable_id) => {
      if let Some(variable) = variables.get(&variable_id) {
        variable.clone()
      } else {
        let variable = Rc::new(RefCell::new(Value::Unit));
        variables.insert(Uuid::new_v4(), variable.clone());
        variable
      }
    }
    Expression::NodeArgument(other_node_parameter) => node_parameters_variables
      .get(&other_node_parameter)
      .ok_or(BehaviorTreeError::InconsistentTreeError {
        message: format!(
          "node argument {} used by node argument {} was not found",
          other_node_parameter,
          node_parameter.to_owned()
        ),
      })?
      .to_owned(),
    Expression::Call(_) => Rc::new(RefCell::new(Value::Unit)),
  };
  node_parameters_variables.insert(node_parameter.to_owned(), variable.to_owned());
  Ok(variable)
}

fn compute_expression(
  variables: &HashMap<Uuid, Rc<RefCell<Value>>>,
  node_parameters_variables: &HashMap<NodeParameterId, Rc<RefCell<Value>>>,
  expression: &Expression,
  caller: &mut dyn CallBridge,
  node_parameter: &NodeParameterId,
) -> Result<Value, BehaviorTreeError> {
  let value = match expression {
    Expression::Value(value) => value.to_owned(),
    Expression::Uuid(uuid) => Value::ArrayU8(uuid.as_bytes().to_vec()),
    Expression::Variable(variable) => variable.borrow().to_owned(),
    Expression::VariableId(variable_id) => {
      let variable = get_variable(&variables, &variable_id, &node_parameter.node)?;
      variable.borrow().to_owned()
    }
    Expression::Call(call) => call_expression(
      &variables,
      &node_parameters_variables,
      call,
      caller,
      &node_parameter,
    )?,
    Expression::NodeArgument(other_node_parameter) => {
      let variable =
        get_node_parameter_variable(&other_node_parameter, &node_parameters_variables)?;
      variable.borrow().to_owned()
    }
  };
  Ok(value)
}

fn compute_uuid(
  variables: &HashMap<Uuid, Rc<RefCell<Value>>>,
  node_parameters_variables: &HashMap<NodeParameterId, Rc<RefCell<Value>>>,
  expression: &Expression,
  caller: &mut dyn CallBridge,
  node_parameter: &NodeParameterId,
) -> Result<Uuid, BehaviorTreeError> {
  match expression {
    Expression::Value(value) => try_into_uuid(&value, &None),
    Expression::Uuid(uuid) => Ok(uuid.to_owned()),
    Expression::Variable(variable) => try_into_uuid(&*variable.borrow(), &None),
    Expression::VariableId(variable_id) => {
      let variable = get_variable(&variables, &variable_id, &node_parameter.node)?;
      try_into_uuid(&*variable.borrow(), &Some(&variable_id))
    }
    Expression::Call(call) => {
      let value = call_expression(
        &variables,
        &node_parameters_variables,
        call,
        caller,
        &node_parameter,
      )?;
      try_into_uuid(&value, &None)
    }
    Expression::NodeArgument(other_node_parameter) => {
      let variable =
        get_node_parameter_variable(&other_node_parameter, &node_parameters_variables)?;
      try_into_uuid(&*variable.borrow(), &None)
    }
  }
}

fn try_into_uuid(value: &Value, variable_id: &Option<&Uuid>) -> Result<Uuid, BehaviorTreeError> {
  if let Value::ArrayU8(uuid_bytes) = value {
    let uuid = Uuid::from_slice(uuid_bytes.as_slice()).map_err(|e| {
      BehaviorTreeError::ConversionError(ConversionError {
        message: format!(
          "bytes of variable {:?} are not an uuid ({})",
          variable_id, e
        ),
      })
    })?;
    Ok(uuid)
  } else {
    Err(BehaviorTreeError::ConversionError(ConversionError {
      message: format!("variable {:?} is not an uuid", variable_id),
    }))
  }
}

fn call_expression(
  variables: &HashMap<Uuid, Rc<RefCell<Value>>>,
  node_arg_variables: &HashMap<NodeParameterId, Rc<RefCell<Value>>>,
  call: &CallExpression,
  caller: &mut dyn CallBridge,
  node_parameter: &NodeParameterId,
) -> Result<Value, BehaviorTreeError> {
  let module_id = compute_uuid(
    &variables,
    &node_arg_variables,
    &call.module,
    caller,
    node_parameter,
  )?;
  let function_id = compute_uuid(
    &variables,
    &node_arg_variables,
    &call.function,
    caller,
    node_parameter,
  )?;
  let mut args = Vec::with_capacity(call.arguments.len());
  for (arg_id_expression, value_expression) in &call.arguments {
    let arg_id = compute_uuid(
      &variables,
      &node_arg_variables,
      &arg_id_expression,
      caller,
      &node_parameter,
    )?;
    let value = compute_expression(
      &variables,
      &node_arg_variables,
      &value_expression,
      caller,
      &node_parameter,
    )?;
    args.push(StructureField {
      id: arg_id,
      value: Box::new(value),
    });
  }
  let result = caller
    .arora_call(
      &module_id,
      Call {
        id: function_id,
        args,
      },
    )
    .map_err(|e| BehaviorTreeError::CallError(e))?;
  Ok(result.ret)
}
