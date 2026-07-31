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
use arora_types::record::module::frozen::Function;
use arora_types::record::ty::{FrozenTy, PrimitiveKind};
use arora_types::value::Value;
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
        &|_| None,
        &HashMap::new(),
    )
    .unwrap();
    let cell_b = crate::setup_node_parameter_variable(
        &param_b,
        &Expression::VariableId(var_id),
        &mut variables,
        &mut node_parameters,
        &|_| None,
        &HashMap::new(),
    )
    .unwrap();

    cell_a.set(Value::Boolean(true));
    assert_eq!(
        cell_b.get_or_unit(),
        Value::Boolean(true),
        "two parameters bound to the same variable id must share one cell"
    );
}

/// A `{var}` whose name the resolver knows binds to the data store: the cell and
/// the store key are the same storage, both ways. This is the Direct convention
/// (variable name == store key) the runtime supplies — exercised here against a
/// real [`SimpleDataStore`] so the behavior-tree crate's `Slot` plumbing is
/// covered without an engine.
#[test]
fn resolved_variable_is_store_backed() {
    use crate::schema::{Expression, NodeParameterId};
    use crate::variable::VariableCell;
    use arora_simple_data_store::SimpleDataStore;
    use arora_types::data::{DataStore, Key, StateChange};

    let store = SimpleDataStore::new();
    let resolver = {
        let store = store.clone();
        // `DataStore::slot` already hands back a `Box<dyn Slot>`.
        move |name: &str| Some(store.slot(&Key::from(name)))
    };

    let var_id = Uuid::new_v4();
    let names = HashMap::from([(var_id, "battery.level".to_string())]);

    let mut variables: HashMap<Uuid, VariableCell> = HashMap::new();
    let mut node_parameters: HashMap<NodeParameterId, VariableCell> = HashMap::new();
    let param = NodeParameterId {
        node: Uuid::new_v4(),
        parameter: Uuid::new_v4(),
    };

    let cell = crate::setup_node_parameter_variable(
        &param,
        &Expression::VariableId(var_id),
        &mut variables,
        &mut node_parameters,
        &resolver,
        &names,
    )
    .unwrap();

    // Write through the cell, read through the store.
    cell.set(Value::from(1.0_f64));
    assert_eq!(
        store.read(&[Key::from("battery.level")]),
        vec![Some(Value::from(1.0_f64))],
        "a write through the resolved cell must reach the store key"
    );

    // Write through the store, read through the cell.
    store
        .write(StateChange::set("battery.level", Value::from(2.0_f64)))
        .unwrap();
    assert_eq!(
        cell.get(),
        Some(Value::from(2.0_f64)),
        "a write through the store key must be visible through the resolved cell"
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
    fn arora_call(&mut self, call: Call) -> Result<CallResult, CallError> {
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

/// Poll-on-tick with per-run identity: two long-running leaves run concurrently
/// under `parallel`, each carrying its own status. The tree is re-ticked while
/// either is `Running` (that re-tick *is* the poll-on-tick contract — see
/// docs/async-functions.md), and completing one leaves the other running: each
/// run advances on its own identity, not a shared slot. This is the state model a
/// spawned `say` action needs — here the two runs are distinct scripted leaves,
/// no module and no network. (`polly::say` keys its runs by content only because
/// the module ABI hands it no per-invocation id; a run spawned as a behavior gets
/// that identity from the node, per the reserved-parameter note in the doc.)
#[test]
fn concurrent_runs_advance_independently() {
    let a = Uuid::from_u128(0xA);
    let b = Uuid::from_u128(0xB);

    let statuses: LeafStatuses = Rc::new(RefCell::new(HashMap::from([
        (a, Status::Running),
        (b, Status::Running),
    ])));
    let ticks: LeafTicks = Rc::new(RefCell::new(HashMap::new()));
    let function_index = Rc::new(HashMap::from([
        (a, scripted_leaf_function(a)),
        (b, scripted_leaf_function(b)),
    ]));
    let tree = build(nodes::parallel(vec![scripted_leaf(a), scripted_leaf(b)]));
    let mut bridge = TestBridge::new(statuses.clone(), ticks.clone());

    // Both runs pending → the tree keeps being re-ticked (Running).
    let mut runtime = BehaviorTreeRuntime::setup(&tree, function_index.clone(), &mut bridge, false)
        .expect("setup");
    assert_eq!(runtime.tick().expect("tick"), Status::Running);
    let _ = runtime;

    // Complete run A only. Its status is its own, so B is untouched and the tree
    // is still Running — the two runs never shared a slot.
    statuses.borrow_mut().insert(a, Status::Success);
    let mut runtime = BehaviorTreeRuntime::setup(&tree, function_index.clone(), &mut bridge, false)
        .expect("setup");
    assert_eq!(runtime.tick().expect("tick"), Status::Running);
    let _ = runtime;

    // Complete run B. Both terminal → the tree succeeds.
    statuses.borrow_mut().insert(b, Status::Success);
    let mut runtime = BehaviorTreeRuntime::setup(&tree, function_index.clone(), &mut bridge, false)
        .expect("setup");
    assert_eq!(runtime.tick().expect("tick"), Status::Success);
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
    let _ = runtime;
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
    let _ = runtime;
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
    let _ = runtime;
    assert_eq!(
        *ticks.borrow().get(&first).unwrap_or(&0),
        2,
        "first re-ticked after reset"
    );
}

/// A fresh interpreter hosts the runner scaffold from the start: it accepts
/// edits before anything is loaded, and with nothing grafted the runner ticks
/// no children — the interpreter idles, always `Running`.
#[test]
fn a_fresh_interpreter_accepts_edits_and_idles_while_empty() {
    use crate::behavior::BehaviorTreeInterpreter;
    use arora_behavior::graph::GraphDiff;
    use arora_behavior::{BehaviorContext, BehaviorInterpreter, BehaviorStatus};
    use arora_simple_data_store::SimpleDataStore;

    let mut interpreter = BehaviorTreeInterpreter::new(Rc::new(HashMap::new()));

    // The scaffold is there before anything is loaded: one runner node.
    assert!(interpreter.graph().root.is_some(), "the runner is the root");
    assert_eq!(interpreter.graph().nodes.len(), 1, "just the runner");

    // The empty (no-op) edit is valid.
    interpreter
        .apply(GraphDiff::default())
        .expect("a fresh interpreter accepts a diff");

    // The re-lowering tick sees the bare runner: idle, stay installed.
    let store = SimpleDataStore::new();
    let (statuses, ticks) = (LeafStatuses::default(), LeafTicks::default());
    let mut bridge = TestBridge::new(statuses, ticks);
    let mut ctx = BehaviorContext {
        store: &store,
        call_bridge: &mut bridge,
    };
    let status = interpreter.tick(&mut ctx).expect("the bare scaffold idles");
    assert_eq!(status, BehaviorStatus::Running);
}

/// Task runs: `spawn`/`halt` on the interpreter, and how a run advances
/// alongside the main tree. Each run invokes a scripted leaf (through
/// [`TestBridge`]) whose returned status the test drives between ticks — a
/// stand-in for a real action-behavior whose status decorator publishes its
/// outcome to the run's status key.
mod task_runs {
    use super::{build, LeafStatuses, LeafTicks, TestBridge};
    use crate::arora_generated::behavior_tree::status::Status;
    use crate::behavior::BehaviorTreeInterpreter;
    use crate::nodes;
    use arora_behavior::{BehaviorContext, BehaviorInterpreter, BehaviorStatus, RunPolicy, TaskId};
    use arora_simple_data_store::SimpleDataStore;
    use arora_types::call::Call;
    use arora_types::data::DataStore;
    use arora_types::value::Value;
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::rc::Rc;
    use uuid::Uuid;

    /// A [`TestBridge`] whose one scripted leaf `leaf` returns `status`, plus the
    /// shared cells to drive it (`statuses`) and observe invocations (`ticks`).
    fn scripted(leaf: Uuid, status: Status) -> (TestBridge, LeafStatuses, LeafTicks) {
        let statuses: LeafStatuses = Rc::new(RefCell::new(HashMap::from([(leaf, status)])));
        let ticks: LeafTicks = Rc::new(RefCell::new(HashMap::new()));
        (
            TestBridge::new(statuses.clone(), ticks.clone()),
            statuses,
            ticks,
        )
    }

    /// A [`Call`] a run invokes: `module`/`leaf` identify the scripted leaf.
    fn call_to(module: u128, leaf: Uuid) -> Call {
        Call {
            module_id: Some(Uuid::from_u128(module)),
            id: leaf,
            args: Vec::new(),
        }
    }

    #[test]
    fn a_spawned_run_advances_and_writes_its_status_key() {
        let leaf = Uuid::from_u128(0xA1);
        let (mut bridge, statuses, ticks) = scripted(leaf, Status::Running);
        let store = SimpleDataStore::new();
        let mut interp = BehaviorTreeInterpreter::new(Rc::new(HashMap::new()));

        // The default-reject is overridden: a handle comes back, its keys
        // namespaced under the run id.
        let handle = interp
            .spawn(call_to(0xB0, leaf), RunPolicy::Concurrent)
            .expect("the tree interpreter hosts runs");
        assert!(
            handle.status.path.starts_with("arora/tasks/"),
            "{}",
            handle.status.path
        );
        assert!(
            handle.status.path.ends_with("/status"),
            "{}",
            handle.status.path
        );

        let mut ctx = BehaviorContext {
            store: &store,
            call_bridge: &mut bridge,
        };
        // Spawn does not tick: nothing is written until the next tick.
        assert_eq!(store.read(std::slice::from_ref(&handle.status)), vec![None]);

        // First tick invokes the run once and publishes Running.
        assert_eq!(interp.tick(&mut ctx).unwrap(), BehaviorStatus::Running);
        let running: Value = Status::Running.into();
        assert_eq!(
            store.read(std::slice::from_ref(&handle.status)),
            vec![Some(running)],
            "the run's status key holds Running"
        );
        assert_eq!(*ticks.borrow().get(&leaf).unwrap(), 1);

        // The run reaches Success: the interpreter publishes it, drops the run,
        // and — no runs, no tree — idles rather than being dropped.
        statuses.borrow_mut().insert(leaf, Status::Success);
        assert_eq!(interp.tick(&mut ctx).unwrap(), BehaviorStatus::Running);
        let success: Value = Status::Success.into();
        assert_eq!(
            store.read(std::slice::from_ref(&handle.status)),
            vec![Some(success)]
        );

        // A finished run is not ticked again.
        let before = *ticks.borrow().get(&leaf).unwrap();
        interp.tick(&mut ctx).unwrap();
        assert_eq!(
            *ticks.borrow().get(&leaf).unwrap(),
            before,
            "a finished run is not re-ticked"
        );
    }

    #[test]
    fn halting_a_run_terminates_it_next_tick() {
        let leaf = Uuid::from_u128(0xC2);
        let (mut bridge, _statuses, ticks) = scripted(leaf, Status::Running);
        let store = SimpleDataStore::new();
        let mut interp = BehaviorTreeInterpreter::new(Rc::new(HashMap::new()));

        let handle = interp
            .spawn(call_to(0xB0, leaf), RunPolicy::Concurrent)
            .unwrap();
        let mut ctx = BehaviorContext {
            store: &store,
            call_bridge: &mut bridge,
        };

        // Runs indefinitely: after a tick it is still Running.
        assert_eq!(interp.tick(&mut ctx).unwrap(), BehaviorStatus::Running);
        let running: Value = Status::Running.into();
        assert_eq!(
            store.read(std::slice::from_ref(&handle.status)),
            vec![Some(running)]
        );

        // Halt: the next tick ends the run (Failure) without invoking it again.
        interp.halt(handle.id).unwrap();
        let ticks_before = *ticks.borrow().get(&leaf).unwrap();
        assert_eq!(interp.tick(&mut ctx).unwrap(), BehaviorStatus::Running);
        let failure: Value = Status::Failure.into();
        assert_eq!(
            store.read(std::slice::from_ref(&handle.status)),
            vec![Some(failure)],
            "a halted run ends Failure"
        );
        assert_eq!(
            *ticks.borrow().get(&leaf).unwrap(),
            ticks_before,
            "a halted run is not invoked"
        );

        // Idempotent: halting the finished run, or an unknown run, is a clean
        // no-op.
        interp
            .halt(handle.id)
            .expect("halting a finished run is a no-op");
        interp
            .halt(TaskId(Uuid::from_u128(0xDEAD)))
            .expect("halting an unknown run is a no-op");
    }

    /// A loaded behavior and a spawned run coexist under the runner and both
    /// advance each tick — the main behavior reactively re-evaluated, the run
    /// invoked once per tick — and the interpreter is a standing policy: it
    /// reports `Running` every tick, never `Done`.
    #[test]
    fn a_loaded_behavior_and_a_run_tick_together() {
        use arora_behavior::graph::{Graph, Node as GraphNode};

        let main_leaf = Uuid::from_u128(0xE3);
        let run_leaf = Uuid::from_u128(0xE4);
        let statuses: LeafStatuses = Rc::new(RefCell::new(HashMap::from([
            (main_leaf, Status::Success),
            (run_leaf, Status::Running),
        ])));
        let ticks: LeafTicks = Rc::new(RefCell::new(HashMap::new()));
        let mut bridge = TestBridge::new(statuses.clone(), ticks.clone());
        let store = SimpleDataStore::new();
        let mut interp = BehaviorTreeInterpreter::new(Rc::new(HashMap::new()));

        // The main behavior: a single run-call leaf graph invoking the
        // scripted main_leaf (dispatched through arora_call, like a module
        // action node would be).
        let main_node = Uuid::from_u128(0x111);
        let mut main = Graph::empty();
        main.root = Some(main_node);
        main.nodes.insert(
            main_node,
            GraphNode {
                id: main_node,
                function: crate::nodes::RUN_CALL_FUNCTION_ID,
                inputs: vec![arora_behavior::graph::Io::new(
                    crate::nodes::RUN_CALL_PARAM_ID,
                )],
                ..GraphNode::default()
            },
        );
        main.links.push(arora_behavior::graph::Link::new(
            arora_behavior::graph::Port::new(main_node, crate::nodes::RUN_CALL_PARAM_ID),
            arora_behavior::graph::LinkSource::Literal(
                arora_types::value_serde::to_value(&call_to(0xB0, main_leaf)).unwrap(),
            ),
        ));
        interp.load(main).expect("the behavior loads");
        interp
            .spawn(call_to(0xB0, run_leaf), RunPolicy::Concurrent)
            .unwrap();

        let mut ctx = BehaviorContext {
            store: &store,
            call_bridge: &mut bridge,
        };
        for round in 1..=3u32 {
            assert_eq!(
                interp.tick(&mut ctx).unwrap(),
                BehaviorStatus::Running,
                "the interpreter is a standing policy (never Done)"
            );
            assert_eq!(
                *ticks.borrow().get(&main_leaf).unwrap(),
                round,
                "the main behavior re-evaluates every tick"
            );
            assert_eq!(
                *ticks.borrow().get(&run_leaf).unwrap(),
                round,
                "the run advances every tick"
            );
        }
    }

    /// The scaffold is introspectable the behavior tree's way: a spawned run is
    /// two graph nodes under the runner (a run-status decorator, its status key
    /// predetermined, over a run-call leaf carrying the call as literal data) —
    /// and a halted run's fragment is pruned again.
    #[test]
    fn a_spawned_run_is_visible_in_the_graph_until_halted() {
        let leaf = Uuid::from_u128(0xE5);
        let (mut bridge, _statuses, _ticks) = scripted(leaf, Status::Running);
        let store = SimpleDataStore::new();
        let mut interp = BehaviorTreeInterpreter::new(Rc::new(HashMap::new()));

        let handle = interp
            .spawn(call_to(0xB0, leaf), RunPolicy::Concurrent)
            .unwrap();
        let graph = interp.graph();
        let decorator = graph
            .nodes
            .values()
            .find(|n| n.function == crate::nodes::RUN_STATUS_FUNCTION_ID)
            .expect("the run's status decorator is in the graph");
        assert_eq!(
            decorator.outputs[0].predetermined_key.as_deref(),
            Some(handle.status.path.as_str()),
            "the decorator's status output is predetermined to the run's key"
        );
        let call_node = decorator.children.as_ref().unwrap()[0];
        assert_eq!(
            graph.nodes[&call_node].function,
            crate::nodes::RUN_CALL_FUNCTION_ID,
            "the decorator wraps the run-call leaf"
        );
        let root = graph.root.expect("the runner roots the scaffold");
        assert!(
            graph.nodes[&root]
                .children
                .as_ref()
                .unwrap()
                .contains(&decorator.id),
            "the fragment hangs off the runner"
        );

        // Halt: the next tick prunes the fragment from the graph.
        let mut ctx = BehaviorContext {
            store: &store,
            call_bridge: &mut bridge,
        };
        interp.tick(&mut ctx).unwrap();
        interp.halt(handle.id).unwrap();
        interp.tick(&mut ctx).unwrap();
        assert_eq!(
            interp.graph().nodes.len(),
            1,
            "only the runner remains after the halt pruned the fragment"
        );
    }

    #[test]
    fn concurrent_runs_get_independent_status_keys() {
        let a = Uuid::from_u128(0xAA);
        let b = Uuid::from_u128(0xBB);
        let statuses: LeafStatuses = Rc::new(RefCell::new(HashMap::from([
            (a, Status::Running),
            (b, Status::Running),
        ])));
        let ticks: LeafTicks = Rc::new(RefCell::new(HashMap::new()));
        let mut bridge = TestBridge::new(statuses.clone(), ticks.clone());
        let store = SimpleDataStore::new();
        let mut interp = BehaviorTreeInterpreter::new(Rc::new(HashMap::new()));

        let ha = interp
            .spawn(call_to(0xB0, a), RunPolicy::Concurrent)
            .unwrap();
        let hb = interp
            .spawn(call_to(0xB0, b), RunPolicy::Concurrent)
            .unwrap();
        assert_ne!(
            ha.status.path, hb.status.path,
            "each run gets its own status key"
        );

        let mut ctx = BehaviorContext {
            store: &store,
            call_bridge: &mut bridge,
        };
        interp.tick(&mut ctx).unwrap();
        let running: Value = Status::Running.into();
        assert_eq!(
            store.read(std::slice::from_ref(&ha.status)),
            vec![Some(running.clone())]
        );
        assert_eq!(
            store.read(std::slice::from_ref(&hb.status)),
            vec![Some(running.clone())]
        );

        // Halt A only; B keeps running independently.
        interp.halt(ha.id).unwrap();
        interp.tick(&mut ctx).unwrap();
        let failure: Value = Status::Failure.into();
        assert_eq!(
            store.read(std::slice::from_ref(&ha.status)),
            vec![Some(failure)],
            "the halted run ended"
        );
        assert_eq!(
            store.read(std::slice::from_ref(&hb.status)),
            vec![Some(running)],
            "the other run is unaffected"
        );
    }
}
