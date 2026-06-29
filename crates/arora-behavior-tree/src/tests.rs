//! Engine-free tests. Tests that need a real engine moved to `arora-sdk`.
//!
//! The basic control nodes (seq, seq_star, fallback, parallel, succeed, fail,
//! run) are dispatched natively, so their execution can be exercised here with a
//! minimal registry-backed [`CallBridge`] — no wasm module, no engine.
use crate::arora_generated::behavior_tree::status::Status;
use crate::load_behavior_tree_yaml;
use crate::nodes;
use crate::tree_node::TreeNode;
use crate::{run_behavior_tree, BehaviorTree, BehaviorTreeRuntime, ModuleFunction};
use anyhow::Result;
use arora_types::call::{Call, CallBridge, CallError, CallResult, Callable, CallableId};
use arora_types::value::Value;
use semio_record::module::v0::frozen::Function;
use semio_record::ty::{FrozenTy, PrimitiveKind};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use uuid::Uuid;

#[test]
pub fn load_parse_error() -> Result<()> {
    let tree_yaml = "I'm singing in the rain...";
    assert!(load_behavior_tree_yaml(tree_yaml).is_err());
    Ok(())
}

#[test]
pub fn load_simple_tree() -> Result<()> {
    let tree_yaml = &crate::schema::tests::SIMPLE_TREE_YAML;
    load_behavior_tree_yaml(tree_yaml)?;
    Ok(())
}

/// Two node parameters that reference the **same** variable id must resolve to a
/// single shared cell, so a write through one is visible through the other —
/// that is what makes `{var}` a shared blackboard entry across nodes.
///
/// Regression test for the variable-sharing bug: a freshly-referenced variable
/// was inserted into the variables map under a fresh `Uuid::new_v4()` instead of
/// its own `variable_id`, so the next lookup by `variable_id` missed and a second,
/// independent cell was created.
#[test]
fn shared_variable_id_resolves_to_one_cell() {
    use crate::schema::{Expression, NodeParameterId};
    use crate::variable::VariableCell;

    let var_id = Uuid::new_v4();
    let mut variables: HashMap<Uuid, VariableCell> = HashMap::new();
    let mut node_parameters: HashMap<NodeParameterId, VariableCell> = HashMap::new();

    let param_a = NodeParameterId {
        node: Uuid::new_v4(),
        parameter: Uuid::new_v4(),
    };
    let param_b = NodeParameterId {
        node: Uuid::new_v4(),
        parameter: Uuid::new_v4(),
    };

    let cell_a = crate::setup_node_parameter_variable(
        &param_a,
        &Expression::VariableId(var_id),
        &mut variables,
        &mut node_parameters,
    )
    .unwrap();
    let cell_b = crate::setup_node_parameter_variable(
        &param_b,
        &Expression::VariableId(var_id),
        &mut variables,
        &mut node_parameters,
    )
    .unwrap();

    cell_a.set(Value::Boolean(true));
    assert_eq!(
        cell_b.get_or_unit(),
        Value::Boolean(true),
        "two parameters bound to the same variable id must share one cell"
    );
}

// Native execution harness
//================================================================
/// A leaf whose status is supplied by a shared, mutable cell, so a test can
/// change what it returns between ticks. Modeled as a plain (non-control) node
/// so it dispatches through `arora_call` — the path real module leaves use.
type LeafStatuses = Rc<RefCell<HashMap<Uuid, Status>>>;

/// Records how many times each leaf function id was ticked.
type LeafTicks = Rc<RefCell<HashMap<Uuid, u32>>>;

/// A [`CallBridge`] that registers/invokes callables natively and answers
/// `arora_call` for the test's scripted leaf functions. Native behavior trees
/// never call into real modules.
struct TestBridge {
    registered: HashMap<u64, Rc<dyn Callable>>,
    next_id: u64,
    leaf_statuses: LeafStatuses,
    leaf_ticks: LeafTicks,
}

impl TestBridge {
    fn new(leaf_statuses: LeafStatuses, leaf_ticks: LeafTicks) -> Self {
        Self {
            registered: HashMap::new(),
            next_id: 0,
            leaf_statuses,
            leaf_ticks,
        }
    }

    fn empty() -> Self {
        Self::new(
            Rc::new(RefCell::new(HashMap::new())),
            Rc::new(RefCell::new(HashMap::new())),
        )
    }
}

impl CallBridge for TestBridge {
    fn arora_call(&mut self, _module: &Uuid, call: Call) -> Result<CallResult, CallError> {
        *self.leaf_ticks.borrow_mut().entry(call.id).or_insert(0) += 1;
        let status = self
            .leaf_statuses
            .borrow()
            .get(&call.id)
            .cloned()
            .ok_or(CallError::FunctionNotFound { id: call.id })?;
        Ok(CallResult {
            ret: status.into(),
            mutated: Vec::new(),
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
        let callable = self
            .registered
            .get(&callable_id.id)
            .cloned()
            .ok_or(CallError::Generic {
                message: format!("unknown callable {}", callable_id.id),
            })?;
        callable.call(self)
    }
}

/// A scripted leaf node: a non-control node dispatched through `arora_call`,
/// whose status comes from the bridge's `leaf_statuses`.
fn scripted_leaf(function: Uuid) -> TreeNode {
    TreeNode {
        function,
        children: None,
        parameters: HashMap::new(),
    }
}

/// A minimal `ModuleFunction` for a scripted leaf, so the native tick path can
/// build the (empty) call. The leaf has no parameters and returns a status.
fn scripted_leaf_function(function: Uuid) -> ModuleFunction {
    ModuleFunction {
        module_id: Uuid::nil(),
        function_id: function,
        function_name: "scripted_leaf".to_string(),
        function: Function {
            parameters: HashMap::new(),
            parameter_ordering: Vec::new(),
            return_ty: FrozenTy::from(PrimitiveKind::U8),
        },
    }
}

fn build(node: TreeNode) -> BehaviorTree {
    node.try_into().expect("tree builds")
}

/// Tick a tree (with only native nodes) exactly once.
fn tick_once(tree: &BehaviorTree) -> Status {
    let mut bridge = TestBridge::empty();
    let mut runtime = BehaviorTreeRuntime::setup(tree, Rc::new(HashMap::new()), &mut bridge, false)
        .expect("runtime sets up");
    runtime.tick().expect("tick succeeds")
}

/// Run a tree (with only native nodes) to a terminal status.
fn run(tree: &BehaviorTree) -> Status {
    let mut bridge = TestBridge::empty();
    run_behavior_tree(tree, Rc::new(HashMap::new()), &mut bridge, false).expect("run succeeds")
}

// seq
//----------------------------------------------------------------
#[test]
fn seq_all_success_is_success() {
    let tree = build(nodes::seq(vec![nodes::succeed(), nodes::succeed()]));
    assert_eq!(run(&tree), Status::Success);
}

#[test]
fn seq_first_failure_is_failure() {
    // `fail()` is native, so it short-circuits before the later child can run.
    // Make the later child a leaf that has no scripted status, so reaching it
    // would surface as an error rather than a silent pass.
    let later = Uuid::from_u128(0xA1);
    let tree = build(nodes::seq(vec![nodes::fail(), scripted_leaf(later)]));
    assert_eq!(tick_once(&tree), Status::Failure);
}

#[test]
fn seq_running_child_is_running() {
    let tree = build(nodes::seq(vec![nodes::succeed(), nodes::run()]));
    assert_eq!(tick_once(&tree), Status::Running);
}

// fallback
//----------------------------------------------------------------
#[test]
fn fallback_first_success_is_success() {
    let tree = build(nodes::fallback(vec![nodes::succeed(), nodes::fail()]));
    assert_eq!(run(&tree), Status::Success);
}

#[test]
fn fallback_all_failure_is_failure() {
    let tree = build(nodes::fallback(vec![nodes::fail(), nodes::fail()]));
    assert_eq!(run(&tree), Status::Failure);
}

#[test]
fn fallback_empty_is_success() {
    let tree = build(nodes::fallback(vec![]));
    assert_eq!(run(&tree), Status::Success);
}

#[test]
fn fallback_running_child_is_running() {
    let tree = build(nodes::fallback(vec![nodes::run(), nodes::succeed()]));
    assert_eq!(tick_once(&tree), Status::Running);
}

// parallel
//----------------------------------------------------------------
#[test]
fn parallel_all_success_is_success() {
    let tree = build(nodes::parallel(vec![nodes::succeed(), nodes::succeed()]));
    assert_eq!(run(&tree), Status::Success);
}

#[test]
fn parallel_any_failure_is_failure() {
    let tree = build(nodes::parallel(vec![
        nodes::succeed(),
        nodes::fail(),
        nodes::succeed(),
    ]));
    assert_eq!(run(&tree), Status::Failure);
}

#[test]
fn parallel_mixed_running_is_running() {
    // A running child with no failing child yields Running.
    let tree = build(nodes::parallel(vec![nodes::succeed(), nodes::run()]));
    assert_eq!(tick_once(&tree), Status::Running);
}

// seq_star
//----------------------------------------------------------------
#[test]
fn seq_star_resumes_and_resets() {
    // Three scripted leaves; the middle one starts as Running. seq_star should
    // tick the first (Success), hit the second (Running) and stop there,
    // remembering index 1. On the next tick it must resume at the second leaf
    // WITHOUT re-ticking the first. After a terminal result the index resets.
    let first = Uuid::from_u128(0x1);
    let second = Uuid::from_u128(0x2);
    let third = Uuid::from_u128(0x3);

    let statuses: LeafStatuses = Rc::new(RefCell::new(HashMap::from([
        (first, Status::Success),
        (second, Status::Running),
        (third, Status::Success),
    ])));
    let ticks: LeafTicks = Rc::new(RefCell::new(HashMap::new()));

    let function_index = Rc::new(HashMap::from([
        (first, scripted_leaf_function(first)),
        (second, scripted_leaf_function(second)),
        (third, scripted_leaf_function(third)),
    ]));

    let tree = build(nodes::seq_star(vec![
        scripted_leaf(first),
        scripted_leaf(second),
        scripted_leaf(third),
    ]));

    let mut bridge = TestBridge::new(statuses.clone(), ticks.clone());

    // First tick: first succeeds, second is Running -> tree Running, index = 1.
    let mut runtime = BehaviorTreeRuntime::setup(&tree, function_index.clone(), &mut bridge, false)
        .expect("setup");
    assert_eq!(runtime.tick().expect("tick"), Status::Running);
    drop(runtime);
    assert_eq!(*ticks.borrow().get(&first).unwrap_or(&0), 1);
    assert_eq!(*ticks.borrow().get(&second).unwrap_or(&0), 1);
    assert_eq!(*ticks.borrow().get(&third).unwrap_or(&0), 0);

    // The second leaf now succeeds.
    statuses.borrow_mut().insert(second, Status::Success);

    // Second tick: resumes at the second leaf (no re-tick of the first), both
    // remaining leaves succeed -> whole seq_star succeeds, index resets to 0.
    let mut runtime = BehaviorTreeRuntime::setup(&tree, function_index.clone(), &mut bridge, false)
        .expect("setup");
    assert_eq!(runtime.tick().expect("tick"), Status::Success);
    drop(runtime);
    assert_eq!(
        *ticks.borrow().get(&first).unwrap_or(&0),
        1,
        "first not re-ticked"
    );
    assert_eq!(*ticks.borrow().get(&second).unwrap_or(&0), 2);
    assert_eq!(*ticks.borrow().get(&third).unwrap_or(&0), 1);

    // Third tick: after the reset, it restarts from the first leaf.
    let mut runtime = BehaviorTreeRuntime::setup(&tree, function_index.clone(), &mut bridge, false)
        .expect("setup");
    assert_eq!(runtime.tick().expect("tick"), Status::Success);
    drop(runtime);
    assert_eq!(
        *ticks.borrow().get(&first).unwrap_or(&0),
        2,
        "first re-ticked after reset"
    );
}
