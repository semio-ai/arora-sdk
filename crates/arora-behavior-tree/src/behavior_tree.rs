// Generated code: lint hygiene is the generator's responsibility, not this
// repo's. Allow clippy/dead_code over the whole generated subtree.
#[allow(clippy::all, dead_code)]
pub mod arora_generated;
pub mod error;
pub mod nodes;
pub mod schema;
pub mod schema_groot;
#[cfg(test)]
mod tests;
pub mod tree_node;
use arora_generated::behavior_tree::{status::Status, tick_id::TickId};
use arora_types::call::{CallBridge, CallError, Callable, CallableId};
use arora_types::{
    call::Call,
    value::{ConversionError, StructureField, Value},
};
use error::BehaviorTreeError;
use schema::{CallExpression, Expression, Node, NodeParameterId, _RET_PARAM_ID};
use semio_record::module::v0::frozen::Function;
use std::{cell::RefCell, collections::HashMap, rc::Rc};
use uuid::Uuid;

use crate::arora_generated::behavior_tree::tick_id::{
    TICK_ID_CALLABLE_ID_FIELD_RAW_ID, TICK_ID_STRUCT_RAW_ID,
};
use crate::nodes::{
    FAIL_FUNCTION_ID, FALLBACK_FUNCTION_ID, PARALLEL_FUNCTION_ID, RUN_FUNCTION_ID, SEQ_FUNCTION_ID,
    SEQ_STAR_CURRENT_INDEX_PARAM_ID, SEQ_STAR_FUNCTION_ID, SUCCEED_FUNCTION_ID,
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

pub struct BehaviorTreeRuntime<'a> {
    caller: &'a mut dyn CallBridge,
    tick: TickId,
}

impl<'a> BehaviorTreeRuntime<'a> {
    pub fn setup(
        tree: &'a BehaviorTree,
        function_index: Rc<HashMap<Uuid, ModuleFunction>>,
        caller: &'a mut dyn CallBridge,
        trace: bool,
    ) -> Result<Self, BehaviorTreeError> {
        let tick = setup_tick_function(
            tree.root.clone(),
            &tree.node_index,
            function_index.clone(),
            tree.variables.clone(),
            tree.node_arg_variables.clone(),
            caller,
            if trace {
                TraceTick::YesAll
            } else {
                TraceTick::No
            },
        )?;
        Ok(Self { caller, tick })
    }

    pub fn tick(&mut self) -> Result<Status, BehaviorTreeError> {
        self.tick.tick(self.caller)
    }
}

/// Runs a behavior tree until it reaches the status success or failure.
pub fn run_behavior_tree(
    behavior: &BehaviorTree,
    function_index: Rc<HashMap<Uuid, ModuleFunction>>,
    caller: &mut dyn CallBridge,
    trace: bool,
) -> Result<Status, BehaviorTreeError> {
    let mut runtime = BehaviorTreeRuntime::setup(behavior, function_index, caller, trace)?;
    let mut status = Status::Running;
    while status == Status::Running {
        status = runtime.tick()?;
    }
    Ok(status)
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum TraceTick {
    YesAll,
    No,
}

fn setup_tick_function(
    node: Rc<Node>,
    node_index: &HashMap<Uuid, Rc<Node>>,
    function_index: Rc<HashMap<Uuid, ModuleFunction>>,
    variables: Rc<RefCell<HashMap<Uuid, Rc<RefCell<Value>>>>>,
    node_arg_variables: Rc<HashMap<NodeParameterId, Rc<RefCell<Value>>>>,
    caller: &mut dyn CallBridge,
    trace: TraceTick,
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
                    node: node.id,
                    child: *child_id,
                })?
                .clone();
            let tick_function_with_id = setup_tick_function(
                child_node.clone(),
                node_index,
                function_index.clone(),
                variables.clone(),
                node_arg_variables.clone(),
                caller,
                trace,
            )?;
            children_ticks.push(tick_function_with_id);
        }
    }
    let tick_function: Rc<dyn Callable> = Rc::new(TickFunction {
        node: node.clone(),
        function_index,
        locals: variables.to_owned(),
        node_arg_variables: node_arg_variables.to_owned(),
        children: children_ticks,
        trace,
    });
    let callable_id = caller.arora_register_callable(tick_function);
    Ok(TickId {
        callable_id: callable_id.id,
    })
}

/// Tick the basic control nodes (seq, seq_star, fallback, parallel, succeed,
/// fail, run) natively, without consulting the function index or the wasm
/// engine. Children are ticked through their registered tick functions
/// ([`Tickable for TickId`]). Returns `Some(status)` for a built-in node and
/// `None` for any other function (which the caller dispatches via the engine).
fn tick_builtin(
    caller: &mut dyn CallBridge,
    node_parameters_variables: &HashMap<NodeParameterId, Rc<RefCell<Value>>>,
    child_tick_ids: &[TickId],
    node: &Node,
) -> Result<Option<Status>, BehaviorTreeError> {
    let status = match node.function {
        SUCCEED_FUNCTION_ID => Status::Success,
        FAIL_FUNCTION_ID => Status::Failure,
        RUN_FUNCTION_ID => Status::Running,
        SEQ_FUNCTION_ID => {
            let mut status = Status::Success;
            for child in child_tick_ids {
                match child.tick(caller)? {
                    Status::Success => continue,
                    Status::Failure => {
                        status = Status::Failure;
                        break;
                    }
                    Status::Running => {
                        status = Status::Running;
                        break;
                    }
                }
            }
            status
        }
        FALLBACK_FUNCTION_ID => {
            if child_tick_ids.is_empty() {
                Status::Success
            } else {
                let mut status = Status::Failure;
                for child in child_tick_ids {
                    match child.tick(caller)? {
                        Status::Success => {
                            status = Status::Success;
                            break;
                        }
                        Status::Failure => continue,
                        Status::Running => {
                            status = Status::Running;
                            break;
                        }
                    }
                }
                status
            }
        }
        PARALLEL_FUNCTION_ID => {
            // Ticks every child every time, then aggregates: Success only if
            // all children succeeded, Failure if any failed, else Running.
            let mut success = true;
            let mut failure = false;
            for child in child_tick_ids {
                match child.tick(caller)? {
                    Status::Success => continue,
                    Status::Failure => {
                        success = false;
                        failure = true;
                    }
                    Status::Running => {
                        success = false;
                    }
                }
            }
            if success {
                Status::Success
            } else if failure {
                Status::Failure
            } else {
                Status::Running
            }
        }
        SEQ_STAR_FUNCTION_ID => {
            // The current index persists in the node's parameter variable, so
            // a Running tree resumes past the children that already succeeded.
            let index_variable = get_node_parameter_variable(
                &NodeParameterId {
                    node: node.id,
                    parameter: SEQ_STAR_CURRENT_INDEX_PARAM_ID,
                },
                node_parameters_variables,
            )?;
            let mut current_index = match *index_variable.borrow() {
                Value::U16(index) => index,
                _ => 0,
            };
            let mut status = Status::Success;
            for child in child_tick_ids.iter().skip(current_index as usize) {
                match child.tick(caller)? {
                    Status::Success => current_index += 1,
                    Status::Failure => {
                        status = Status::Failure;
                        break;
                    }
                    Status::Running => {
                        status = Status::Running;
                        break;
                    }
                }
            }
            if status != Status::Running {
                current_index = 0;
            }
            *index_variable.borrow_mut() = Value::U16(current_index);
            status
        }
        _ => return Ok(None),
    };
    Ok(Some(status))
}

fn tick(
    caller: &mut dyn CallBridge,
    function_index: Rc<HashMap<Uuid, ModuleFunction>>,
    variables: Rc<RefCell<HashMap<Uuid, Rc<RefCell<Value>>>>>,
    node_parameters_variables: Rc<HashMap<NodeParameterId, Rc<RefCell<Value>>>>,
    child_tick_ids: &Vec<TickId>,
    node: Rc<Node>,
    trace: TraceTick,
) -> Result<Status, BehaviorTreeError> {
    // The basic control nodes are wired in natively; everything else is
    // dispatched into a module through the engine.
    if let Some(status) = tick_builtin(caller, &node_parameters_variables, child_tick_ids, &node)? {
        if trace != TraceTick::No {
            println!("tick {} -> {:?}", node.id, status);
        }
        return Ok(status);
    }

    let module_function =
        function_index
            .get(&node.function)
            .ok_or(BehaviorTreeError::CallError(CallError::FunctionNotFound {
                id: node.function,
            }))?;
    let function = &module_function.function;

    let mut call = Call {
        module_id: None,
        id: node.function,
        args: Vec::with_capacity(
            node.arguments.len() + if node.children.is_some() { 1 } else { 0 },
        ),
    };

    let nof_children = node
        .children
        .as_ref()
        .map(|children| children.len())
        .unwrap_or(0);
    assert_eq!(nof_children, child_tick_ids.len());

    if node.children.is_some() {
        // Check the presence of the `children` parameter, which must be the first one.
        let first_parameter_id = function.parameter_ordering.first().ok_or(
            BehaviorTreeError::MissingChildrenParameter {
                node: node.id.to_owned(),
                function: node.function.to_owned(),
            },
        )?;
        let first_parameter = function.parameters.get(first_parameter_id).ok_or(
            BehaviorTreeError::MissingChildrenParameter {
                node: node.id,
                function: node.function.to_owned(),
            },
        )?;
        if first_parameter.name != "children" {
            Err(BehaviorTreeError::MissingChildrenParameter {
                node: node.id,
                function: node.function.to_owned(),
            })?;
        }
        let first_parameter_type_id = first_parameter
            .ty
            .as_array()
            .ok_or(BehaviorTreeError::MissingChildrenParameter {
                node: node.id,
                function: node.function.to_owned(),
            })?
            .reference
            .id;
        if first_parameter_type_id != Uuid::from_bytes(TICK_ID_STRUCT_RAW_ID) {
            Err(BehaviorTreeError::MissingChildrenParameter {
                node: node.id,
                function: node.function.to_owned(),
            })?;
        }

        // Pass the tick ids of the children.
        let mut children_arg = Vec::with_capacity(child_tick_ids.len());
        for child_tick_id in child_tick_ids {
            children_arg.push(arora_types::value::StructureWithoutId {
                fields: vec![StructureField {
                    id: Uuid::from_bytes(TICK_ID_CALLABLE_ID_FIELD_RAW_ID),
                    value: Box::new(Value::U64(child_tick_id.callable_id)),
                }],
            });
        }
        call.args.push(StructureField {
            id: *first_parameter_id,
            value: Box::new(Value::ArrayStructure {
                id: Uuid::from_bytes(TICK_ID_STRUCT_RAW_ID),
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
            let variable =
                get_node_parameter_variable(&node_parameter, &node_parameters_variables)?;

            // If the parameter expression is a call, perform the call to update the value.
            if let Expression::Call(parameter_call_expression) = value_expression {
                let value = call_expression(
                    &variables,
                    &node_parameters_variables,
                    parameter_call_expression,
                    caller,
                    &NodeParameterId {
                        node: node.id,
                        parameter: param_id.to_owned(),
                    },
                )?;
                *variable.borrow_mut() = value;
            };

            locals.insert(*param_id, variable.clone());
            if param_id == &_RET_PARAM_ID {
                continue;
            }
            call.args.push(StructureField {
                id: *param_id,
                value: Box::new(variable.borrow().clone()),
            });
        }
    }

    let result = caller
        .arora_call(&module_function.module_id, call)
        .map_err(BehaviorTreeError::CallError)?;

    for mutated in result.mutated {
        let variable = locals
            .get_mut(&mutated.id)
            .ok_or(BehaviorTreeError::InternalError {
                message: format!(
                    "mutated parameter {} does not correspond to any local variable",
                    mutated.id
                ),
            })?;
        *variable.borrow_mut() = *mutated.value;
    }

    // If the node has a _ret argument (function does not return a status),
    // write the return value to the bound variable.
    let status = if let Some(var) = locals.get(&_RET_PARAM_ID) {
        *var.borrow_mut() = result.ret.clone();
        Status::Success
    } else {
        result.ret.try_into().unwrap_or(Status::Success)
    };
    if trace != TraceTick::No {
        println!("tick {} -> {:?}", node.id, status);
    }
    Ok(status)
}

/// Specialization of Callable that returns a Status.
trait Tickable {
    fn tick(&self, caller: &mut dyn CallBridge) -> Result<Status, BehaviorTreeError>;
}

impl Tickable for TickId {
    fn tick(&self, caller: &mut dyn CallBridge) -> Result<Status, BehaviorTreeError> {
        CallableId {
            id: self.callable_id,
        }
        .tick(caller)
    }
}

impl Tickable for CallableId {
    fn tick(&self, caller: &mut dyn CallBridge) -> Result<Status, BehaviorTreeError> {
        let value = self.call(caller).map_err(BehaviorTreeError::CallError)?;
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
    function_index: Rc<HashMap<Uuid, ModuleFunction>>,
    locals: Rc<RefCell<HashMap<Uuid, Rc<RefCell<Value>>>>>,
    node_arg_variables: Rc<HashMap<NodeParameterId, Rc<RefCell<Value>>>>,
    children: Vec<TickId>,
    trace: TraceTick,
}

impl Tickable for TickFunction {
    fn tick(&self, caller: &mut dyn CallBridge) -> Result<Status, BehaviorTreeError> {
        tick(
            caller,
            self.function_index.clone(),
            self.locals.clone(),
            self.node_arg_variables.clone(),
            &self.children,
            self.node.clone(),
            self.trace,
        )
    }
}

impl Callable for TickFunction {
    fn call(&self, caller: &mut dyn CallBridge) -> Result<Value, CallError> {
        self.tick(caller)
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
        let existing_node = node_index.insert(shared_node.id, shared_node.clone());
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
                arg_expr,
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

pub fn load_behavior_tree_yaml(yaml: &str) -> Result<BehaviorTree, BehaviorTreeError> {
    load_behavior_tree_nodes(serde_yaml::from_str(yaml)?)
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
        .get(node_parameter)
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
            if let Some(variable) = variables.get(variable_id) {
                variable.clone()
            } else {
                let variable = Rc::new(RefCell::new(Value::Unit));
                variables.insert(Uuid::new_v4(), variable.clone());
                variable
            }
        }
        Expression::NodeArgument(other_node_parameter) => node_parameters_variables
            .get(other_node_parameter)
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
            let variable = get_variable(variables, variable_id, &node_parameter.node)?;
            variable.borrow().to_owned()
        }
        Expression::Call(call) => call_expression(
            variables,
            node_parameters_variables,
            call,
            caller,
            node_parameter,
        )?,
        Expression::NodeArgument(other_node_parameter) => {
            let variable =
                get_node_parameter_variable(other_node_parameter, node_parameters_variables)?;
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
        Expression::Value(value) => try_into_uuid(value, &None),
        Expression::Uuid(uuid) => Ok(uuid.to_owned()),
        Expression::Variable(variable) => try_into_uuid(&variable.borrow(), &None),
        Expression::VariableId(variable_id) => {
            let variable = get_variable(variables, variable_id, &node_parameter.node)?;
            try_into_uuid(&variable.borrow(), &Some(variable_id))
        }
        Expression::Call(call) => {
            let value = call_expression(
                variables,
                node_parameters_variables,
                call,
                caller,
                node_parameter,
            )?;
            try_into_uuid(&value, &None)
        }
        Expression::NodeArgument(other_node_parameter) => {
            let variable =
                get_node_parameter_variable(other_node_parameter, node_parameters_variables)?;
            try_into_uuid(&variable.borrow(), &None)
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
        variables,
        node_arg_variables,
        &call.module,
        caller,
        node_parameter,
    )?;
    let function_id = compute_uuid(
        variables,
        node_arg_variables,
        &call.function,
        caller,
        node_parameter,
    )?;
    let mut args = Vec::with_capacity(call.arguments.len());
    for (arg_id_expression, value_expression) in &call.arguments {
        let arg_id = compute_uuid(
            variables,
            node_arg_variables,
            arg_id_expression,
            caller,
            node_parameter,
        )?;
        let value = compute_expression(
            variables,
            node_arg_variables,
            value_expression,
            caller,
            node_parameter,
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
                module_id: None,
                id: function_id,
                args,
            },
        )
        .map_err(BehaviorTreeError::CallError)?;
    Ok(result.ret)
}

pub struct ModuleFunction {
    pub module_id: Uuid,
    pub function_id: Uuid,
    pub function_name: String,
    pub function: Function,
}
