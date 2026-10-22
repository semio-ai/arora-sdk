//! Call expressions as dataflow: functions of several modules nested in a
//! node's argument, evaluated innermost first each time the node ticks.
//!
//! A fake engine hosts four modules:
//!
//! | module | functions |
//! |---|---|
//! | `sensors` | `distance() -> f64`, `battery() -> f64`: what the test sets |
//! | `math` | `add(a, b)`, `mul(a, b)`, `clamp(x, lo, hi: Option<f64>)` |
//! | `filters` | `smooth(x, alpha, state: &mut f64) -> f64`: `state += alpha * (x - state)` |
//! | `checks` | `below(x, limit) -> Status`: `Success` when `x < limit` |
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use arora_behavior::{interpreter_module, FunctionIndex, ModuleFunction, Status};
use arora_types::call::{Call, CallBridge, CallError, CallResult, Callable, CallableId};
use arora_types::record::module::frozen::{Function, Parameter};
use arora_types::record::ty::{FrozenOption, FrozenTy, PrimitiveKind};
use arora_types::value::{StructureField, Value};
use uuid::Uuid;

use crate::display::Names;
use crate::nodes::{
    EQUAL_A_PARAM_ID, EQUAL_B_PARAM_ID, EQUAL_FUNCTION_ID, PARALLEL_FUNCTION_ID, SEQ_FUNCTION_ID,
};
use crate::schema::{CallExpression, Expression, Node, NodeParameterId, _RET_PARAM_ID};
use crate::{load_behavior_tree_nodes, BehaviorTree, BehaviorTreeRuntime};

pub(crate) const SENSORS: Uuid = Uuid::from_u128(0x5E00);
pub(crate) const DISTANCE: Uuid = Uuid::from_u128(0x5E01);
pub(crate) const BATTERY: Uuid = Uuid::from_u128(0x5E02);
pub(crate) const LABEL: Uuid = Uuid::from_u128(0x5E03);
pub(crate) const RECORD: Uuid = Uuid::from_u128(0x5E04);
pub(crate) const RECORD_NAME: Uuid = Uuid::from_u128(0x5E41);

pub(crate) const MATH: Uuid = Uuid::from_u128(0x3A00);
pub(crate) const ADD: Uuid = Uuid::from_u128(0x3A01);
pub(crate) const ADD_A: Uuid = Uuid::from_u128(0x3A11);
pub(crate) const ADD_B: Uuid = Uuid::from_u128(0x3A12);
pub(crate) const MUL: Uuid = Uuid::from_u128(0x3A02);
pub(crate) const MUL_A: Uuid = Uuid::from_u128(0x3A21);
pub(crate) const MUL_B: Uuid = Uuid::from_u128(0x3A22);
pub(crate) const CLAMP: Uuid = Uuid::from_u128(0x3A03);
pub(crate) const CLAMP_X: Uuid = Uuid::from_u128(0x3A31);
pub(crate) const CLAMP_LO: Uuid = Uuid::from_u128(0x3A32);
pub(crate) const CLAMP_HI: Uuid = Uuid::from_u128(0x3A33);

pub(crate) const FILTERS: Uuid = Uuid::from_u128(0xF100);
pub(crate) const SMOOTH: Uuid = Uuid::from_u128(0xF101);
pub(crate) const SMOOTH_X: Uuid = Uuid::from_u128(0xF111);
pub(crate) const SMOOTH_ALPHA: Uuid = Uuid::from_u128(0xF112);
pub(crate) const SMOOTH_STATE: Uuid = Uuid::from_u128(0xF113);
pub(crate) const TWIN: Uuid = Uuid::from_u128(0xF102);
pub(crate) const KEEP: Uuid = Uuid::from_u128(0xF103);
pub(crate) const KEEP_STATE: Uuid = Uuid::from_u128(0xF131);
pub(crate) const TWIN_FIRST: Uuid = Uuid::from_u128(0xF121);
pub(crate) const TWIN_SECOND: Uuid = Uuid::from_u128(0xF122);

pub(crate) const CHECKS: Uuid = Uuid::from_u128(0xC400);
pub(crate) const BELOW: Uuid = Uuid::from_u128(0xC401);
pub(crate) const BELOW_X: Uuid = Uuid::from_u128(0xC411);
pub(crate) const BELOW_LIMIT: Uuid = Uuid::from_u128(0xC412);

pub(crate) fn f64_ty() -> FrozenTy {
    FrozenTy::from(PrimitiveKind::F64)
}

pub(crate) fn parameter(name: &str, ty: FrozenTy, mutable: bool) -> Parameter {
    Parameter {
        name: name.to_string(),
        ty,
        mutable,
    }
}

pub(crate) fn function(
    module: Uuid,
    id: Uuid,
    name: &str,
    parameters: Vec<(Uuid, Parameter)>,
) -> (Uuid, ModuleFunction) {
    (
        id,
        ModuleFunction {
            module_id: module,
            function_id: id,
            function_name: name.to_string(),
            function: Function {
                parameter_ordering: parameters.iter().map(|(id, _)| *id).collect(),
                parameters: parameters.into_iter().collect(),
                return_ty: f64_ty(),
            },
        },
    )
}

/// The four modules' functions, as the device's index describes them.
pub(crate) fn index() -> FunctionIndex {
    let number = |name| parameter(name, f64_ty(), false);
    let optional = FrozenTy::FrozenOption(FrozenOption {
        element: Box::new(f64_ty()),
    });
    FunctionIndex::from([
        function(SENSORS, DISTANCE, "distance", vec![]),
        function(SENSORS, BATTERY, "battery", vec![]),
        function(SENSORS, LABEL, "label", vec![]),
        function(SENSORS, RECORD, "record", vec![]),
        function(
            MATH,
            ADD,
            "add",
            vec![(ADD_A, number("a")), (ADD_B, number("b"))],
        ),
        function(
            MATH,
            MUL,
            "mul",
            vec![(MUL_A, number("a")), (MUL_B, number("b"))],
        ),
        function(
            MATH,
            CLAMP,
            "clamp",
            vec![
                (CLAMP_X, number("x")),
                (CLAMP_LO, number("lo")),
                (CLAMP_HI, parameter("hi", optional, false)),
            ],
        ),
        function(
            FILTERS,
            SMOOTH,
            "smooth",
            vec![
                (SMOOTH_X, number("x")),
                (SMOOTH_ALPHA, number("alpha")),
                (SMOOTH_STATE, parameter("state", f64_ty(), true)),
            ],
        ),
        function(
            FILTERS,
            TWIN,
            "twin",
            vec![
                (TWIN_FIRST, parameter("first", f64_ty(), true)),
                (TWIN_SECOND, parameter("second", f64_ty(), true)),
            ],
        ),
        function(
            FILTERS,
            KEEP,
            "keep",
            vec![(KEEP_STATE, parameter("state", f64_ty(), true))],
        ),
        function(
            CHECKS,
            BELOW,
            "below",
            vec![(BELOW_X, number("x")), (BELOW_LIMIT, number("limit"))],
        ),
    ])
}

/// What the engine was asked: each call's function name and arguments.
pub(crate) type Calls = Rc<RefCell<Vec<(&'static str, HashMap<Uuid, Value>)>>>;

/// A fake engine hosting the four modules. A call naming a function under
/// another module than its own is not found, as the engine dispatches on both.
#[derive(Default)]
pub(crate) struct Engine {
    registered: HashMap<u64, Rc<dyn Callable>>,
    next_id: u64,
    pub(crate) sensors: Rc<RefCell<HashMap<Uuid, f64>>>,
    pub(crate) calls: Calls,
}

impl Engine {
    pub(crate) fn set(&self, sensor: Uuid, value: f64) {
        self.sensors.borrow_mut().insert(sensor, value);
    }
}

pub(crate) fn number(args: &HashMap<Uuid, Value>, id: Uuid) -> Result<f64, CallError> {
    match args.get(&id) {
        Some(Value::F64(value)) => Ok(*value),
        other => Err(CallError::Generic {
            message: format!("argument {id}: expected an f64, got {other:?}"),
        }),
    }
}

pub(crate) fn returns(value: Value) -> CallResult {
    CallResult {
        ret: value,
        mutated: Vec::new(),
    }
}

impl CallBridge for Engine {
    fn arora_call(&mut self, call: Call) -> Result<CallResult, CallError> {
        let not_found = CallError::FunctionNotFound { id: call.id };
        let Some(function) = index().remove(&call.id) else {
            return Err(not_found);
        };
        if call.module_id != Some(function.module_id) {
            return Err(not_found);
        }
        let args: HashMap<Uuid, Value> = call
            .args
            .into_iter()
            .map(|arg| (arg.id, *arg.value))
            .collect();
        let name: &'static str = Box::leak(function.function_name.into_boxed_str());
        self.calls.borrow_mut().push((name, args.clone()));
        let sensor = |id| Value::F64(self.sensors.borrow().get(&id).copied().unwrap_or(0.0));
        Ok(match call.id {
            DISTANCE => returns(sensor(DISTANCE)),
            BATTERY => returns(sensor(BATTERY)),
            LABEL => returns(Value::String("ready".to_string())),
            RECORD => returns(Value::Structure(arora_types::value::Structure {
                id: RECORD,
                fields: vec![StructureField {
                    id: RECORD_NAME,
                    value: Box::new(Value::String("near".to_string())),
                }],
            })),
            ADD => returns(Value::F64(number(&args, ADD_A)? + number(&args, ADD_B)?)),
            MUL => returns(Value::F64(number(&args, MUL_A)? * number(&args, MUL_B)?)),
            CLAMP => {
                let x = number(&args, CLAMP_X)?.max(number(&args, CLAMP_LO)?);
                let x = match args.get(&CLAMP_HI) {
                    Some(Value::Option(Some(hi))) => match **hi {
                        Value::F64(hi) => x.min(hi),
                        ref other => {
                            return Err(CallError::Generic {
                                message: format!("hi: {other}"),
                            })
                        }
                    },
                    Some(Value::Option(None)) | None => x,
                    Some(other) => {
                        return Err(CallError::Generic {
                            message: format!("hi is optional, got {other}"),
                        })
                    }
                };
                returns(Value::F64(x))
            }
            SMOOTH => {
                let x = number(&args, SMOOTH_X)?;
                let state = number(&args, SMOOTH_STATE)?;
                let state = state + number(&args, SMOOTH_ALPHA)? * (x - state);
                CallResult {
                    ret: Value::F64(state),
                    mutated: vec![StructureField {
                        id: SMOOTH_STATE,
                        value: Box::new(Value::F64(state)),
                    }],
                }
            }
            KEEP => returns(Value::F64(0.0)),
            TWIN => CallResult {
                ret: Value::F64(0.0),
                mutated: [(TWIN_FIRST, 1.0), (TWIN_SECOND, 2.0)]
                    .map(|(id, value)| StructureField {
                        id,
                        value: Box::new(Value::F64(value)),
                    })
                    .into(),
            },
            BELOW => returns(if number(&args, BELOW_X)? < number(&args, BELOW_LIMIT)? {
                Status::Success.into()
            } else {
                Status::Failure.into()
            }),
            _ => return Err(not_found),
        })
    }

    fn arora_register_callable(&mut self, callable: Rc<dyn Callable>) -> CallableId {
        let id = self.next_id;
        self.next_id += 1;
        self.registered.insert(id, callable);
        CallableId { id }
    }

    fn arora_unregister_callable(&mut self, callable_id: &CallableId) {
        self.registered.remove(&callable_id.id);
    }

    fn arora_call_indirect(&mut self, callable_id: &CallableId) -> Result<Value, CallError> {
        let callable = self.registered[&callable_id.id].clone();
        callable.call(self)
    }
}

// Building expressions and nodes.
//================================================================
pub(crate) fn f(value: f64) -> Expression {
    Expression::Value(Value::F64(value))
}

/// `function(arguments…)`, under the module the index names for it.
pub(crate) fn call(function: Uuid, arguments: Vec<(Uuid, Expression)>) -> Expression {
    let module = index()[&function].module_id;
    Expression::Call(Box::new(CallExpression {
        module: Expression::Uuid(module),
        function: Expression::Uuid(function),
        arguments: arguments
            .into_iter()
            .map(|(parameter, argument)| (Expression::Uuid(parameter), argument))
            .collect(),
    }))
}

pub(crate) fn node(id: u128, function: Uuid, arguments: Vec<(Uuid, Expression)>) -> Node {
    Node {
        id: Uuid::from_u128(id),
        function,
        arguments: arguments.into_iter().collect(),
        children: None,
    }
}

pub(crate) fn control(id: u128, function: Uuid, children: &[u128]) -> Node {
    Node {
        id: Uuid::from_u128(id),
        function,
        arguments: HashMap::new(),
        children: Some(children.iter().copied().map(Uuid::from_u128).collect()),
    }
}

pub(crate) fn variable(name: u128) -> Expression {
    Expression::VariableId(Uuid::from_u128(name))
}

/// `clamp(x: mul(a: distance(), b: 100.0), lo: 0.0)`: the distance in
/// centimetres, never negative. Three modules, three levels deep.
pub(crate) fn centimetres() -> Expression {
    call(
        CLAMP,
        vec![
            (
                CLAMP_X,
                call(
                    MUL,
                    vec![(MUL_A, call(DISTANCE, vec![])), (MUL_B, f(100.0))],
                ),
            ),
            (CLAMP_LO, f(0.0)),
        ],
    )
}

pub(crate) fn tree(nodes: Vec<Node>) -> BehaviorTree {
    load_behavior_tree_nodes(nodes).expect("the tree builds")
}

/// The value a tree variable holds.
pub(crate) fn read(tree: &BehaviorTree, name: u128) -> Option<Value> {
    tree.variables.borrow()[&Uuid::from_u128(name)].get()
}

// Tests.
//================================================================

/// A condition leaf's input is computed by a cascade of functions from three
/// modules, innermost first, every tick: the tree's status follows the sensor.
#[test]
fn a_cascade_across_modules_feeds_a_condition() {
    let tree = tree(vec![node(
        1,
        BELOW,
        vec![(BELOW_X, centimetres()), (BELOW_LIMIT, f(30.0))],
    )]);
    let mut engine = Engine::default();
    let sensors = engine.sensors.clone();
    let calls = engine.calls.clone();
    let mut runtime =
        BehaviorTreeRuntime::setup(&tree, &index(), &mut engine, false).expect("it lowers");

    for (distance, status) in [
        (0.2, Status::Success),  // 20 cm
        (0.5, Status::Failure),  // 50 cm
        (-0.1, Status::Success), // clamped to 0 cm
    ] {
        sensors.borrow_mut().insert(DISTANCE, distance);
        assert_eq!(runtime.tick().expect("it ticks"), status, "at {distance} m");
        let order: Vec<_> = calls.borrow_mut().drain(..).map(|(name, _)| name).collect();
        assert_eq!(order, ["distance", "mul", "clamp", "below"]);
    }
}

/// A cascade's result reaches a variable through the leaf's return binding,
/// and a later leaf of the sequence reads it: a named edge, so the value is
/// computed once and read by any number of nodes.
#[test]
fn a_cascade_writes_a_variable_a_later_leaf_reads() {
    const LEVEL: u128 = 0x1E7E1;
    // {level} = add(a: mul(a: battery(), b: 0.5), b: clamp(…distance…))
    let level = node(
        2,
        ADD,
        vec![
            (
                ADD_A,
                call(MUL, vec![(MUL_A, call(BATTERY, vec![])), (MUL_B, f(0.5))]),
            ),
            (ADD_B, centimetres()),
            (_RET_PARAM_ID, variable(LEVEL)),
        ],
    );
    let check = node(
        3,
        BELOW,
        vec![(BELOW_X, variable(LEVEL)), (BELOW_LIMIT, f(50.0))],
    );
    let tree = tree(vec![control(1, SEQ_FUNCTION_ID, &[2, 3]), level, check]);
    let mut engine = Engine::default();
    engine.set(BATTERY, 80.0);
    engine.set(DISTANCE, 0.05);
    let calls = engine.calls.clone();
    let mut runtime =
        BehaviorTreeRuntime::setup(&tree, &index(), &mut engine, false).expect("it lowers");

    // 80 * 0.5 + 5 = 45 < 50.
    assert_eq!(runtime.tick().unwrap(), Status::Success);
    assert_eq!(read(&tree, LEVEL), Some(Value::F64(45.0)));
    let order: Vec<_> = calls.borrow_mut().drain(..).map(|(name, _)| name).collect();
    assert_eq!(
        order,
        ["battery", "mul", "distance", "mul", "clamp", "add", "below"],
        "below reads the variable: the cascade is not computed again"
    );
}

/// A literal fed to a mutable parameter is the call site's state: it starts
/// from the literal and keeps what the function writes back, and two call
/// sites of one function keep two states.
#[test]
fn a_call_site_keeps_the_state_of_its_mutable_parameter() {
    const FAST: u128 = 0xFA57;
    const SLOW: u128 = 0x510;
    let smoothed = |alpha, target| {
        node(
            target,
            CLAMP,
            vec![
                (
                    CLAMP_X,
                    call(
                        SMOOTH,
                        vec![
                            (SMOOTH_X, call(DISTANCE, vec![])),
                            (SMOOTH_ALPHA, f(alpha)),
                            (SMOOTH_STATE, f(0.0)),
                        ],
                    ),
                ),
                (CLAMP_LO, f(0.0)),
                (_RET_PARAM_ID, variable(target)),
            ],
        )
    };
    let tree = tree(vec![
        control(1, PARALLEL_FUNCTION_ID, &[FAST, SLOW]),
        smoothed(0.5, FAST),
        smoothed(0.25, SLOW),
    ]);
    let mut engine = Engine::default();
    engine.set(DISTANCE, 1.0);
    let mut runtime =
        BehaviorTreeRuntime::setup(&tree, &index(), &mut engine, false).expect("it lowers");

    for (fast, slow) in [(0.5, 0.25), (0.75, 0.4375), (0.875, 0.578125)] {
        runtime.tick().unwrap();
        assert_eq!(read(&tree, FAST), Some(Value::F64(fast)));
        assert_eq!(read(&tree, SLOW), Some(Value::F64(slow)));
    }
}

/// A mutable parameter fed a variable keeps its state in that variable, where
/// any node reads it: here two call sites share one state.
#[test]
fn a_mutable_parameter_fed_a_variable_writes_back_to_it() {
    const STATE: u128 = 0x57A7E;
    let smooth = |id| {
        node(
            id,
            CLAMP,
            vec![
                (
                    CLAMP_X,
                    call(
                        SMOOTH,
                        vec![
                            (SMOOTH_X, f(1.0)),
                            (SMOOTH_ALPHA, f(0.5)),
                            (SMOOTH_STATE, variable(STATE)),
                        ],
                    ),
                ),
                (CLAMP_LO, f(0.0)),
            ],
        )
    };
    let tree = tree(vec![
        control(1, SEQ_FUNCTION_ID, &[2, 3]),
        smooth(2),
        smooth(3),
    ]);
    tree.variables.borrow()[&Uuid::from_u128(STATE)].set(Value::F64(0.0));
    let mut engine = Engine::default();
    let mut runtime =
        BehaviorTreeRuntime::setup(&tree, &index(), &mut engine, false).expect("it lowers");

    runtime.tick().unwrap();
    // Each of the two call sites moved the shared state half way to 1.
    assert_eq!(read(&tree, STATE), Some(Value::F64(0.75)));
}

/// An absent value passes `None` to an optional parameter, and a value passes
/// `Some`, as a module leaf frames them: the call is framed by the signature
/// the tree was lowered with.
#[test]
fn an_optional_parameter_receives_none_or_some() {
    const HI: u128 = 0x41;
    let tree = tree(vec![node(
        1,
        BELOW,
        vec![
            (
                BELOW_X,
                call(
                    CLAMP,
                    vec![
                        (CLAMP_X, f(80.0)),
                        (CLAMP_LO, f(0.0)),
                        (CLAMP_HI, variable(HI)),
                    ],
                ),
            ),
            (BELOW_LIMIT, f(50.0)),
        ],
    )]);
    let mut engine = Engine::default();
    let calls = engine.calls.clone();
    let mut runtime =
        BehaviorTreeRuntime::setup(&tree, &index(), &mut engine, false).expect("it lowers");
    let hi_received = || {
        calls
            .borrow()
            .iter()
            .find(|(name, _)| *name == "clamp")
            .map(|(_, args)| args[&CLAMP_HI].clone())
    };

    assert_eq!(
        runtime.tick().unwrap(),
        Status::Failure,
        "80 is not below 50"
    );
    assert_eq!(hi_received(), Some(Value::Option(None)));

    calls.borrow_mut().clear();
    tree.variables.borrow()[&Uuid::from_u128(HI)].set(Value::F64(40.0));
    assert_eq!(runtime.tick().unwrap(), Status::Success, "clamped to 40");
    assert_eq!(
        hi_received(),
        Some(Value::Option(Some(Box::new(Value::F64(40.0)))))
    );
}

/// A native node's argument computed by a call keeps the call's result in its
/// cell, which another node reads through a node argument.
#[test]
fn a_native_node_keeps_its_call_s_result_for_a_node_argument() {
    let equal = node(
        2,
        EQUAL_FUNCTION_ID,
        vec![
            (EQUAL_A_PARAM_ID, centimetres()),
            (EQUAL_B_PARAM_ID, f(20.0)),
        ],
    );
    let check = node(
        3,
        BELOW,
        vec![
            (
                BELOW_X,
                Expression::NodeArgument(NodeParameterId {
                    node: Uuid::from_u128(2),
                    parameter: EQUAL_A_PARAM_ID,
                }),
            ),
            (BELOW_LIMIT, f(30.0)),
        ],
    );
    let tree = tree(vec![control(1, SEQ_FUNCTION_ID, &[2, 3]), equal, check]);
    let mut engine = Engine::default();
    engine.set(DISTANCE, 0.2);
    let calls = engine.calls.clone();
    let mut runtime =
        BehaviorTreeRuntime::setup(&tree, &index(), &mut engine, false).expect("it lowers");

    assert_eq!(runtime.tick().unwrap(), Status::Success);
    let below = calls.borrow().last().cloned().expect("below is called");
    assert_eq!(below.0, "below");
    assert_eq!(below.1[&BELOW_X], Value::F64(20.0));
}

/// A tree built again from the same nodes takes over each call site's state.
#[test]
fn a_rebuilt_tree_keeps_its_call_sites_state() {
    const OUT: u128 = 0x07;
    let nodes = || {
        vec![node(
            1,
            CLAMP,
            vec![
                (
                    CLAMP_X,
                    call(
                        SMOOTH,
                        vec![
                            (SMOOTH_X, f(1.0)),
                            (SMOOTH_ALPHA, f(0.5)),
                            (SMOOTH_STATE, f(0.0)),
                        ],
                    ),
                ),
                (CLAMP_LO, f(0.0)),
                (_RET_PARAM_ID, variable(OUT)),
            ],
        )]
    };
    let first = tree(nodes());
    let mut engine = Engine::default();
    BehaviorTreeRuntime::setup(&first, &index(), &mut engine, false)
        .unwrap()
        .tick()
        .unwrap();
    assert_eq!(read(&first, OUT), Some(Value::F64(0.5)));

    let second = tree(nodes());
    second.keep_state(&first, &Default::default());
    BehaviorTreeRuntime::setup(&second, &index(), &mut engine, false)
        .unwrap()
        .tick()
        .unwrap();
    assert_eq!(read(&second, OUT), Some(Value::F64(0.75)), "it goes on");
}

fn lowering_error(argument: Expression) -> String {
    let tree = tree(vec![node(
        1,
        BELOW,
        vec![(BELOW_X, argument), (BELOW_LIMIT, f(30.0))],
    )]);
    let mut engine = Engine::default();
    match BehaviorTreeRuntime::setup(&tree, &index(), &mut engine, false) {
        Ok(_) => panic!("the tree lowers"),
        Err(error) => error.to_string(),
    }
}

/// Lowering refuses what a call's signature rules out, and names the call in
/// the text form.
#[test]
fn lowering_refuses_a_call_its_signature_rules_out() {
    let unknown = Uuid::from_u128(0xBAD);
    let error = lowering_error(call(MUL, vec![(MUL_A, f(1.0)), (unknown, f(2.0))]));
    assert!(
        error.contains(&format!("mul(a: {{= 1f64}}, {unknown}: {{= 2f64}})"))
            && error.contains("does not have"),
        "{error}"
    );

    let error = lowering_error(call(
        SMOOTH,
        vec![
            (SMOOTH_X, f(1.0)),
            (SMOOTH_ALPHA, f(0.5)),
            (SMOOTH_STATE, call(BATTERY, vec![])),
        ],
    ));
    assert!(
        error.contains("mutable parameter `state` with battery()"),
        "{error}"
    );
}

/// A call to the interpreter module is refused: when lowering, for a stated
/// function, and when ticking, for a computed one. The engine is not called.
#[test]
fn a_call_to_the_interpreter_module_is_refused() {
    let load = |function| {
        Expression::Call(Box::new(CallExpression {
            module: Expression::Uuid(interpreter_module::ID),
            function,
            arguments: vec![],
        }))
    };
    let error = lowering_error(load(Expression::Uuid(interpreter_module::LOAD)));
    assert!(error.contains("calls the interpreter module"), "{error}");

    // The function id is computed: it reads the bytes of `LOAD` from the
    // root's argument, so only the tick sees it.
    const ID_PARAM: Uuid = Uuid::from_u128(0x1D);
    let mut root = node(
        1,
        BELOW,
        vec![
            (
                BELOW_X,
                load(Expression::NodeArgument(NodeParameterId {
                    node: Uuid::from_u128(1),
                    parameter: ID_PARAM,
                })),
            ),
            (BELOW_LIMIT, f(30.0)),
        ],
    );
    root.arguments
        .insert(ID_PARAM, Expression::Uuid(interpreter_module::LOAD));
    let tree = tree(vec![root]);
    let mut engine = Engine::default();
    let calls = engine.calls.clone();
    let mut runtime =
        BehaviorTreeRuntime::setup(&tree, &index(), &mut engine, false).expect("it lowers");
    let error = runtime.tick().expect_err("the tick refuses it").to_string();
    assert!(error.contains("which a tree does not call"), "{error}");
    assert!(calls.borrow().is_empty());
}

/// The text form: one line with `{}`, broken over lines past the width with
/// `{:#}`, with names from the index, or ids without them.
#[test]
fn an_expression_shows_as_nested_calls() {
    let functions = index();
    let names = Names::functions(&functions);
    let expression = call(
        BELOW,
        vec![(BELOW_X, centimetres()), (BELOW_LIMIT, f(30.0))],
    );

    assert_eq!(
        expression.display(names).to_string(),
        "below(x: clamp(x: mul(a: distance(), b: 100f64), lo: 0f64), limit: 30f64)"
    );
    assert_eq!(
        format!("{:#}", expression.display(names)),
        "below(\n    x: clamp(x: mul(a: distance(), b: 100f64), lo: 0f64),\n    limit: 30f64,\n)"
    );
    assert_eq!(
        call(DISTANCE, vec![]).to_string(),
        format!("{SENSORS}::{DISTANCE}()"),
        "without names, ids"
    );
}

/// A node shows as the call it makes, its return binding in front, its
/// arguments in the order of its function's parameters.
#[test]
fn a_node_shows_as_the_call_it_makes() {
    let functions = index();
    let names = crate::display::Names {
        functions: Some(&functions),
        variables: Some(&HashMap::from([(
            Uuid::from_u128(0x1E7E1),
            "face/level".to_string(),
        )])),
        ..Default::default()
    };
    let level = node(
        2,
        ADD,
        vec![
            (_RET_PARAM_ID, variable(0x1E7E1)),
            (ADD_B, centimetres()),
            (ADD_A, call(BATTERY, vec![])),
        ],
    );
    assert_eq!(
        level.display(names).to_string(),
        "{face/level} = add(a: battery(), b: clamp(x: mul(a: distance(), b: 100f64), lo: 0f64))"
    );
    assert_eq!(
        format!("{:#}", level.display(names)),
        "{face/level} = add(\n    a: battery(),\n    b: clamp(x: mul(a: distance(), b: 100f64), lo: 0f64),\n)"
    );
}

/// A core string node computes a call-valued argument into its cell before it
/// reads it, as every other node does.
#[test]
fn a_string_node_computes_a_call_argument() {
    const NAME: u128 = 0x4A3E;
    let tree = tree(vec![node(
        1,
        crate::nodes::SET_STR_FUNCTION_ID,
        vec![
            (crate::nodes::SET_STR_VARIABLE_PARAM_ID, variable(NAME)),
            (crate::nodes::SET_STR_VALUE_PARAM_ID, call(LABEL, vec![])),
        ],
    )]);
    let mut engine = Engine::default();
    let calls = engine.calls.clone();
    let status = BehaviorTreeRuntime::setup(&tree, &index(), &mut engine, false)
        .expect("it lowers")
        .tick()
        .expect("it ticks");
    assert_eq!(status, Status::Success);
    assert_eq!(read(&tree, NAME), Some(Value::String("ready".to_string())));
    assert_eq!(calls.borrow().len(), 1, "label is called once");
}

/// A core string node reads a selection of a call's result: the call is made,
/// and the node reads the selected value.
#[test]
fn a_string_node_reads_a_selection_of_a_call() {
    const NAME: u128 = 0x4A3F;
    let name = Expression::Select {
        source: Box::new(call(RECORD, vec![])),
        path: arora_types::data::Key::new(format!(".{RECORD_NAME}")),
    };
    let tree = tree(vec![node(
        1,
        crate::nodes::SET_STR_FUNCTION_ID,
        vec![
            (crate::nodes::SET_STR_VARIABLE_PARAM_ID, variable(NAME)),
            (crate::nodes::SET_STR_VALUE_PARAM_ID, name),
        ],
    )]);
    let mut engine = Engine::default();
    let status = BehaviorTreeRuntime::setup(&tree, &index(), &mut engine, false)
        .expect("it lowers")
        .tick()
        .expect("it ticks");
    assert_eq!(status, Status::Success);
    assert_eq!(read(&tree, NAME), Some(Value::String("near".to_string())));
}

/// A tree that does not lower leaves nothing registered in the engine: the
/// ticks registered before the refusal are unregistered.
#[test]
fn a_refused_tree_leaves_no_tick_registered() {
    let unknown = Uuid::from_u128(0xBAD);
    let tree = tree(vec![
        control(1, SEQ_FUNCTION_ID, &[2, 3]),
        node(2, BELOW, vec![(BELOW_X, f(1.0)), (BELOW_LIMIT, f(2.0))]),
        node(
            3,
            BELOW,
            vec![
                (BELOW_X, call(MUL, vec![(MUL_A, f(1.0)), (unknown, f(2.0))])),
                (BELOW_LIMIT, f(2.0)),
            ],
        ),
    ]);
    let mut engine = Engine::default();
    assert!(BehaviorTreeRuntime::setup(&tree, &index(), &mut engine, false).is_err());
    assert!(engine.registered.is_empty(), "every tick is unregistered");
    assert!(crate::lower_behavior_tree(tree, &index(), &mut engine, false).is_err());
    assert!(engine.registered.is_empty(), "every tick is unregistered");
}

/// A mutable parameter whose id a call computes, fed by a call, has no cell to
/// hold what the function writes back: the tick fails rather than drop it.
#[test]
fn a_mutation_with_no_cell_to_hold_it_fails_the_tick() {
    const STATE_ID: u128 = 0x57A7;
    let Expression::Call(mut smooth) =
        call(SMOOTH, vec![(SMOOTH_X, f(1.0)), (SMOOTH_ALPHA, f(1.0))])
    else {
        unreachable!("call builds a call");
    };
    smooth
        .arguments
        .push((variable(STATE_ID), call(BATTERY, vec![])));
    let tree = tree(vec![node(
        1,
        BELOW,
        vec![(BELOW_X, Expression::Call(smooth)), (BELOW_LIMIT, f(30.0))],
    )]);
    tree.variables.borrow()[&Uuid::from_u128(STATE_ID)]
        .set(Value::ArrayU8(SMOOTH_STATE.as_bytes().to_vec()));
    let mut engine = Engine::default();
    let error = BehaviorTreeRuntime::setup(&tree, &index(), &mut engine, false)
        .expect("it lowers")
        .tick()
        .expect_err("the state has nowhere to go");
    assert!(
        error.to_string().contains("has no cell to hold it"),
        "{error}"
    );
}
