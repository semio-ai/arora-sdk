//! Engine-free tests. Tests that need a real engine moved to `arora-sdk`.
//!
//! The basic control nodes (seq, seq_star, fallback, parallel, succeed, fail,
//! run) are dispatched natively, so their execution can be exercised here with a
//! minimal registry-backed [`CallBridge`] — no wasm module, no engine.
use crate::load_behavior_tree_yaml;
use crate::nodes;
use crate::tree_node::TreeNode;
use crate::{run_behavior_tree, BehaviorTree, BehaviorTreeRuntime, ModuleFunction};
use anyhow::Result;
use arora_behavior::Status;
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

pub(crate) fn build(node: TreeNode) -> BehaviorTree {
    node.try_into().expect("tree builds")
}

/// Tick a tree (with only native nodes) exactly once.
fn tick_once(tree: &BehaviorTree) -> Status {
    let mut bridge = TestBridge::empty();
    let mut runtime = BehaviorTreeRuntime::setup(tree, &HashMap::new(), &mut bridge, false)
        .expect("runtime sets up");
    runtime.tick().expect("tick succeeds")
}

/// Run a tree (with only native nodes) to a terminal status.
fn run(tree: &BehaviorTree) -> Status {
    let mut bridge = TestBridge::empty();
    run_behavior_tree(tree, &HashMap::new(), &mut bridge, false).expect("run succeeds")
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
    let function_index = HashMap::from([
        (a, scripted_leaf_function(a)),
        (b, scripted_leaf_function(b)),
    ]);
    let tree = build(nodes::parallel(vec![scripted_leaf(a), scripted_leaf(b)]));
    let mut bridge = TestBridge::new(statuses.clone(), ticks.clone());

    // Both runs pending → the tree keeps being re-ticked (Running).
    let mut runtime =
        BehaviorTreeRuntime::setup(&tree, &function_index, &mut bridge, false).expect("setup");
    assert_eq!(runtime.tick().expect("tick"), Status::Running);
    let _ = runtime;

    // Complete run A only. Its status is its own, so B is untouched and the tree
    // is still Running — the two runs never shared a slot.
    statuses.borrow_mut().insert(a, Status::Success);
    let mut runtime =
        BehaviorTreeRuntime::setup(&tree, &function_index, &mut bridge, false).expect("setup");
    assert_eq!(runtime.tick().expect("tick"), Status::Running);
    let _ = runtime;

    // Complete run B. Both terminal → the tree succeeds.
    statuses.borrow_mut().insert(b, Status::Success);
    let mut runtime =
        BehaviorTreeRuntime::setup(&tree, &function_index, &mut bridge, false).expect("setup");
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

    let function_index = HashMap::from([
        (first, scripted_leaf_function(first)),
        (second, scripted_leaf_function(second)),
        (third, scripted_leaf_function(third)),
    ]);

    let tree = build(nodes::seq_star(vec![
        scripted_leaf(first),
        scripted_leaf(second),
        scripted_leaf(third),
    ]));

    let mut bridge = TestBridge::new(statuses.clone(), ticks.clone());

    // First tick: first succeeds, second is Running -> tree Running, index = 1.
    let mut runtime =
        BehaviorTreeRuntime::setup(&tree, &function_index, &mut bridge, false).expect("setup");
    assert_eq!(runtime.tick().expect("tick"), Status::Running);
    let _ = runtime;
    assert_eq!(*ticks.borrow().get(&first).unwrap_or(&0), 1);
    assert_eq!(*ticks.borrow().get(&second).unwrap_or(&0), 1);
    assert_eq!(*ticks.borrow().get(&third).unwrap_or(&0), 0);

    // The second leaf now succeeds.
    statuses.borrow_mut().insert(second, Status::Success);

    // Second tick: resumes at the second leaf (no re-tick of the first), both
    // remaining leaves succeed -> whole seq_star succeeds, index resets to 0.
    let mut runtime =
        BehaviorTreeRuntime::setup(&tree, &function_index, &mut bridge, false).expect("setup");
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
    let mut runtime =
        BehaviorTreeRuntime::setup(&tree, &function_index, &mut bridge, false).expect("setup");
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

    let mut interpreter = BehaviorTreeInterpreter::new();

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
        functions: &arora_behavior::FunctionIndex::new(),
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
    use super::{LeafStatuses, LeafTicks, TestBridge};
    use crate::behavior::{graph_type, BehaviorTreeInterpreter};
    use crate::nodes;
    use arora_behavior::Status;
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
        let mut interp = BehaviorTreeInterpreter::new();

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
            functions: &arora_behavior::FunctionIndex::new(),
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
        let mut interp = BehaviorTreeInterpreter::new();

        let handle = interp
            .spawn(call_to(0xB0, leaf), RunPolicy::Concurrent)
            .unwrap();
        let mut ctx = BehaviorContext {
            functions: &arora_behavior::FunctionIndex::new(),
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
        let mut interp = BehaviorTreeInterpreter::new();

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
            functions: &arora_behavior::FunctionIndex::new(),
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
        let mut interp = BehaviorTreeInterpreter::new();

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
            functions: &arora_behavior::FunctionIndex::new(),
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
        let mut interp = BehaviorTreeInterpreter::new();

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
            functions: &arora_behavior::FunctionIndex::new(),
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

    /// A graph that ticks every step and never ends: a parallel of a run-call
    /// leaf invoking `leaf` and a `Run`. Its nodes are `ids` (parallel, call
    /// node, run node).
    fn stepping_graph(ids: [u128; 3], leaf: Uuid) -> arora_behavior::graph::Graph {
        use arora_behavior::graph::{Graph, Io, Link, LinkSource, Node as GraphNode, Port};
        let [parallel, call_node, run] = ids.map(Uuid::from_u128);
        let mut graph = Graph::empty();
        graph.root = Some(parallel);
        graph.nodes.insert(
            parallel,
            GraphNode {
                id: parallel,
                function: nodes::PARALLEL_FUNCTION_ID,
                children: Some(vec![call_node, run]),
                ..GraphNode::default()
            },
        );
        graph.nodes.insert(
            call_node,
            GraphNode {
                id: call_node,
                function: nodes::RUN_CALL_FUNCTION_ID,
                inputs: vec![Io::new(nodes::RUN_CALL_PARAM_ID)],
                ..GraphNode::default()
            },
        );
        graph.nodes.insert(
            run,
            GraphNode {
                id: run,
                function: nodes::RUN_FUNCTION_ID,
                ..GraphNode::default()
            },
        );
        graph.links.push(Link::new(
            Port::new(call_node, nodes::RUN_CALL_PARAM_ID),
            LinkSource::Literal(arora_types::value_serde::to_value(&call_to(0xB0, leaf)).unwrap()),
        ));
        graph
    }

    /// A core string node is native: a graph calling one spawns on a device
    /// with no module, and the run ticks it.
    #[test]
    fn a_spawned_graph_may_call_a_core_string_node() {
        use arora_behavior::graph::{Graph, Io, Link, LinkSource, Node as GraphNode, Port};
        let node = Uuid::from_u128(0x30);
        let mut graph = Graph::empty();
        graph.root = Some(node);
        graph.nodes.insert(
            node,
            GraphNode {
                id: node,
                function: nodes::IS_STR_SET_FUNCTION_ID,
                inputs: vec![Io::new(nodes::IS_STR_SET_VALUE_PARAM_ID)],
                ..GraphNode::default()
            },
        );
        graph.links.push(Link::new(
            Port::new(node, nodes::IS_STR_SET_VALUE_PARAM_ID),
            LinkSource::Literal(Value::String("set".to_string())),
        ));
        let mut interp = BehaviorTreeInterpreter::new();
        let handle = interp
            .spawn_graph(&graph_type(), graph, RunPolicy::Concurrent)
            .expect("a core node needs no module");

        let store = SimpleDataStore::new();
        let mut bridge = TestBridge::new(LeafStatuses::default(), LeafTicks::default());
        let mut ctx = BehaviorContext {
            functions: &arora_behavior::FunctionIndex::new(),
            store: &store,
            call_bridge: &mut bridge,
        };
        interp.tick(&mut ctx).unwrap();
        let success: Value = Status::Success.into();
        assert_eq!(
            store.read(std::slice::from_ref(&handle.status)),
            vec![Some(success)],
            "the run ticked the core node"
        );
    }

    /// A spawned graph is a run: its tree ticks on every step beside the main
    /// behavior, its status key reads Running, and a halt ends it Failure and
    /// prunes every one of its nodes.
    #[test]
    fn a_spawned_behavior_ticks_every_step_until_halted() {
        let leaf = Uuid::from_u128(0xF1);
        let (mut bridge, _statuses, ticks) = scripted(leaf, Status::Success);
        let store = SimpleDataStore::new();
        let mut interp = BehaviorTreeInterpreter::new();

        let handle = interp
            .spawn_graph(
                &graph_type(),
                stepping_graph([0x10, 0x11, 0x12], leaf),
                RunPolicy::Concurrent,
            )
            .expect("the tree interpreter hosts a graph as a run");
        assert_eq!(
            handle.status.path,
            format!(
                "arora/tasks/{}/{}/{}/status",
                arora_behavior::interpreter_module::ID,
                arora_behavior::interpreter_module::SPAWN_GRAPH,
                handle.id.0
            ),
            "the run's keys are under the interpreter module's spawn_graph"
        );

        let mut ctx = BehaviorContext {
            functions: &arora_behavior::FunctionIndex::new(),
            store: &store,
            call_bridge: &mut bridge,
        };
        let running: Value = Status::Running.into();
        for round in 1..=3u32 {
            assert_eq!(interp.tick(&mut ctx).unwrap(), BehaviorStatus::Running);
            assert_eq!(
                *ticks.borrow().get(&leaf).unwrap(),
                round,
                "the run's leaf ticks once per step"
            );
            assert_eq!(
                store.read(std::slice::from_ref(&handle.status)),
                vec![Some(running.clone())]
            );
        }

        interp.halt(handle.id).unwrap();
        interp.tick(&mut ctx).unwrap();
        let failure: Value = Status::Failure.into();
        assert_eq!(
            store.read(std::slice::from_ref(&handle.status)),
            vec![Some(failure)],
            "a halted run ends Failure"
        );
        assert_eq!(
            *ticks.borrow().get(&leaf).unwrap(),
            3,
            "a halted run is not ticked"
        );
        assert_eq!(
            interp.graph().nodes.len(),
            1,
            "only the runner remains after the halt pruned the run's nodes"
        );
    }

    /// An edit reaches a spawned graph's nodes: it relinks a literal and adds a
    /// node under the run, which then belongs to it. Loading a main behavior
    /// leaves the run in place, and halting prunes the node the edit added.
    #[test]
    fn an_edit_reaches_a_spawned_behavior_and_a_load_keeps_it() {
        use arora_behavior::graph::{Graph, GraphDiff, Link, LinkSource, Node as GraphNode, Port};

        let a = Uuid::from_u128(0xA1);
        let b = Uuid::from_u128(0xB1);
        let statuses: LeafStatuses = Rc::new(RefCell::new(HashMap::from([
            (a, Status::Success),
            (b, Status::Success),
        ])));
        let ticks: LeafTicks = Rc::new(RefCell::new(HashMap::new()));
        let mut bridge = TestBridge::new(statuses, ticks.clone());
        let store = SimpleDataStore::new();
        let mut interp = BehaviorTreeInterpreter::new();
        let [parallel, call_node, run] = [0x20, 0x21, 0x22].map(Uuid::from_u128);
        let handle = interp
            .spawn_graph(
                &graph_type(),
                stepping_graph([0x20, 0x21, 0x22], a),
                RunPolicy::Concurrent,
            )
            .unwrap();
        let mut ctx = BehaviorContext {
            functions: &arora_behavior::FunctionIndex::new(),
            store: &store,
            call_bridge: &mut bridge,
        };
        interp.tick(&mut ctx).unwrap();
        assert_eq!(ticks.borrow().get(&a), Some(&1));

        // Relink the run-call leaf's literal: the run now calls `b`.
        interp
            .apply(GraphDiff {
                add_links: vec![Link::new(
                    Port::new(call_node, nodes::RUN_CALL_PARAM_ID),
                    LinkSource::Literal(
                        arora_types::value_serde::to_value(&call_to(0xB0, b)).unwrap(),
                    ),
                )],
                ..GraphDiff::default()
            })
            .expect("the edit applies");
        interp.tick(&mut ctx).unwrap();
        assert_eq!(ticks.borrow().get(&a), Some(&1), "the old call is gone");
        assert_eq!(ticks.borrow().get(&b), Some(&1), "the relinked call runs");

        // Add a node under the run: a second `Run` child of its parallel.
        let added = Uuid::from_u128(0x23);
        interp
            .apply(GraphDiff {
                add_nodes: vec![
                    GraphNode {
                        id: parallel,
                        function: nodes::PARALLEL_FUNCTION_ID,
                        children: Some(vec![call_node, run, added]),
                        ..GraphNode::default()
                    },
                    GraphNode {
                        id: added,
                        function: nodes::RUN_FUNCTION_ID,
                        ..GraphNode::default()
                    },
                ],
                ..GraphDiff::default()
            })
            .expect("the edit applies");

        // Load a main behavior: the run stays and keeps ticking.
        let main_node = Uuid::from_u128(0x30);
        let mut main = Graph::empty();
        main.root = Some(main_node);
        main.nodes.insert(
            main_node,
            GraphNode {
                id: main_node,
                function: nodes::SUCCEED_FUNCTION_ID,
                ..GraphNode::default()
            },
        );
        interp.load(main).expect("the main behavior loads");
        interp.tick(&mut ctx).unwrap();
        assert_eq!(
            ticks.borrow().get(&b),
            Some(&2),
            "the run survived the load"
        );
        for id in [parallel, call_node, run, added, main_node] {
            assert!(
                interp.graph().nodes.contains_key(&id),
                "{id} is in the graph"
            );
        }

        // A second load replaces the main behavior only.
        interp
            .load(Graph::empty())
            .expect("an empty main behavior loads");
        assert!(!interp.graph().nodes.contains_key(&main_node));
        assert!(
            interp.graph().nodes.contains_key(&added),
            "the run's nodes stay"
        );

        // Halt: every node of the run leaves, the one the edit added included.
        interp.halt(handle.id).unwrap();
        interp.tick(&mut ctx).unwrap();
        assert_eq!(
            interp.graph().nodes.len(),
            1,
            "only the runner remains: {:?}",
            interp.graph().nodes.keys().collect::<Vec<_>>()
        );
    }

    /// Runs hang from the runner after the main behavior, in spawn order, and an
    /// edit cannot change a run's decorator or remove a run's root: halting is
    /// how a run leaves.
    #[test]
    fn runs_tick_in_spawn_order_and_keep_their_decorators() {
        use arora_behavior::graph::{GraphDiff, Link, LinkSource, Node as GraphNode, Port};

        let leaf = Uuid::from_u128(0xC1);
        let (mut bridge, _statuses, _ticks) = scripted(leaf, Status::Success);
        let store = SimpleDataStore::new();
        let mut interp = BehaviorTreeInterpreter::new();
        let mut decorators = Vec::new();
        for (n, ids) in [[0x40, 0x41, 0x42], [0x50, 0x51, 0x52], [0x60, 0x61, 0x62]]
            .into_iter()
            .enumerate()
        {
            let handle = interp
                .spawn_graph(
                    &graph_type(),
                    stepping_graph(ids, leaf),
                    RunPolicy::Concurrent,
                )
                .unwrap();
            let decorator = interp
                .graph()
                .nodes
                .values()
                .find(|node| {
                    node.outputs
                        .first()
                        .and_then(|io| io.predetermined_key.as_deref())
                        == Some(handle.status.path.as_str())
                })
                .expect("the run's decorator")
                .id;
            decorators.push(decorator);
            let root = interp.graph().root.unwrap();
            assert_eq!(
                interp.graph().nodes[&root].children.as_deref(),
                Some(decorators.as_slice()),
                "after {} spawns the runner holds the runs in spawn order",
                n + 1
            );
        }

        let refused = [
            GraphDiff {
                remove_nodes: vec![decorators[1]],
                ..GraphDiff::default()
            },
            GraphDiff {
                add_nodes: vec![GraphNode {
                    id: decorators[1],
                    function: nodes::SUCCEED_FUNCTION_ID,
                    ..GraphNode::default()
                }],
                ..GraphDiff::default()
            },
            GraphDiff {
                add_links: vec![Link::new(
                    Port::new(decorators[1], nodes::RUN_STATUS_LATCH_PARAM_ID),
                    LinkSource::Literal(Status::Success.into()),
                )],
                ..GraphDiff::default()
            },
            GraphDiff {
                remove_nodes: vec![Uuid::from_u128(0x50)],
                ..GraphDiff::default()
            },
        ];
        for diff in refused {
            assert!(interp.apply(diff.clone()).is_err(), "{diff:?} is refused");
        }
        let mut ctx = BehaviorContext {
            functions: &arora_behavior::FunctionIndex::new(),
            store: &store,
            call_bridge: &mut bridge,
        };
        interp.tick(&mut ctx).expect("the scaffold still lowers");
        assert!(interp.graph().nodes.contains_key(&decorators[1]));
    }

    /// A run is the subtree under its decorator, whatever order the edits came
    /// in: a node added loose and then placed under a run belongs to the run, so
    /// a load keeps it and a halt prunes it.
    #[test]
    fn a_node_placed_under_a_run_by_a_later_edit_belongs_to_it() {
        use arora_behavior::graph::{Graph, GraphDiff, Node as GraphNode};

        let leaf = Uuid::from_u128(0xC2);
        let (mut bridge, _statuses, _ticks) = scripted(leaf, Status::Success);
        let store = SimpleDataStore::new();
        let mut interp = BehaviorTreeInterpreter::new();
        let [parallel, call_node, run] = [0x90, 0x91, 0x92].map(Uuid::from_u128);
        let handle = interp
            .spawn_graph(
                &graph_type(),
                stepping_graph([0x90, 0x91, 0x92], leaf),
                RunPolicy::Concurrent,
            )
            .unwrap();

        let loose = Uuid::from_u128(0x93);
        interp
            .apply(GraphDiff {
                add_nodes: vec![GraphNode {
                    id: loose,
                    function: nodes::RUN_FUNCTION_ID,
                    ..GraphNode::default()
                }],
                ..GraphDiff::default()
            })
            .unwrap();
        interp
            .apply(GraphDiff {
                add_nodes: vec![GraphNode {
                    id: parallel,
                    function: nodes::PARALLEL_FUNCTION_ID,
                    children: Some(vec![call_node, run, loose]),
                    ..GraphNode::default()
                }],
                ..GraphDiff::default()
            })
            .unwrap();
        interp.load(Graph::empty()).unwrap();
        let mut ctx = BehaviorContext {
            functions: &arora_behavior::FunctionIndex::new(),
            store: &store,
            call_bridge: &mut bridge,
        };
        interp
            .tick(&mut ctx)
            .expect("the run still lowers after the load");
        assert!(
            interp.graph().nodes.contains_key(&loose),
            "the load kept it"
        );

        interp.halt(handle.id).unwrap();
        interp.tick(&mut ctx).unwrap();
        assert_eq!(
            interp.graph().nodes.len(),
            1,
            "the halt pruned it with the run"
        );
    }

    /// A graph that reads `variable`, declared under `name`: a sequence whose
    /// one child takes the variable as an argument.
    fn reading(root: u128, variable: Uuid, name: &str) -> arora_behavior::graph::Graph {
        use arora_behavior::graph::{Graph, Link, LinkSource, Node as GraphNode, Port};
        let root = Uuid::from_u128(root);
        let mut graph = Graph::empty();
        graph.root = Some(root);
        graph.nodes.insert(
            root,
            GraphNode {
                id: root,
                function: nodes::SUCCEED_FUNCTION_ID,
                ..GraphNode::default()
            },
        );
        graph.links.push(Link::new(
            Port::new(root, Uuid::from_u128(0x1)),
            LinkSource::Variable(variable),
        ));
        graph.variables.insert(variable, name.to_string());
        graph
    }

    /// A spawned graph is a region of its own: a graph type the interpreter
    /// reads, one tree of new nodes under its root, links among its own nodes,
    /// and no renaming of a variable the device's behavior reads. A load cannot
    /// take over a run's nodes either.
    #[test]
    fn a_spawned_graph_that_overlaps_the_device_s_is_refused() {
        use arora_behavior::graph::{Graph, GraphType, Link, LinkSource, Node as GraphNode, Port};

        let leaf = Uuid::from_u128(0xD1);
        let mut interp = BehaviorTreeInterpreter::new();
        interp
            .spawn_graph(
                &graph_type(),
                stepping_graph([0x70, 0x71, 0x72], leaf),
                RunPolicy::Concurrent,
            )
            .unwrap();
        let variable = Uuid::from_u128(0x5);
        interp.load(reading(0xA0, variable, "face/x")).unwrap();
        let nodes_before = interp.graph().nodes.len();

        let fresh = || stepping_graph([0x80, 0x81, 0x82], leaf);
        let [parallel, call_node, run] = [0x80, 0x81, 0x82].map(Uuid::from_u128);
        let mut rootless = fresh();
        rootless.root = None;
        let mut stray_root = fresh();
        stray_root.root = Some(Uuid::from_u128(0x99));
        let reused = stepping_graph([0x80, 0x71, 0x82], leaf);
        let mut reaching_out = fresh();
        reaching_out.links.push(Link::new(
            Port::new(Uuid::from_u128(0x71), nodes::RUN_CALL_PARAM_ID),
            LinkSource::Literal(Value::Unit),
        ));
        let mut renaming = fresh();
        renaming.variables.insert(variable, "face/y".to_string());
        let mut dangling_child = fresh();
        dangling_child.nodes.get_mut(&parallel).unwrap().children =
            Some(vec![call_node, run, Uuid::from_u128(0x99)]);
        let mut cycle = fresh();
        cycle.nodes.get_mut(&run).unwrap().children = Some(vec![run]);
        let mut two_parents = fresh();
        two_parents.nodes.get_mut(&run).unwrap().children = Some(vec![call_node]);
        let mut stray_node = fresh();
        stray_node.nodes.insert(
            Uuid::from_u128(0x83),
            GraphNode {
                id: Uuid::from_u128(0x83),
                function: nodes::RUN_FUNCTION_ID,
                ..GraphNode::default()
            },
        );
        let mut child_outside = fresh();
        child_outside.nodes.get_mut(&run).unwrap().children = Some(vec![Uuid::from_u128(0xA0)]);

        let tree = graph_type();
        let ty = |name: &str, major, minor| GraphType {
            name: name.to_string(),
            version: semver::Version::new(major, minor, 0),
        };
        for (what, graph_type, graph) in [
            ("no root", tree.clone(), rootless),
            ("a root that is not a node", tree.clone(), stray_root),
            ("a node id the device uses", tree.clone(), reused),
            ("a link onto another node", tree.clone(), reaching_out),
            ("a variable in use renamed", tree.clone(), renaming),
            ("a child that is not a node", tree.clone(), dangling_child),
            ("a cycle", tree.clone(), cycle),
            ("a node with two parents", tree.clone(), two_parents),
            ("a node outside the tree", tree.clone(), stray_node),
            ("a child of the main behavior", tree.clone(), child_outside),
            ("another language", ty("node-graph", 1, 0), fresh()),
            ("another major", ty("behavior-tree", 2, 0), fresh()),
            ("a newer minor", ty("behavior-tree", 1, 99), fresh()),
        ] {
            let refused = interp.spawn_graph(&graph_type, graph, RunPolicy::Concurrent);
            assert!(refused.is_err(), "a graph with {what} is refused");
        }
        assert_eq!(
            interp.graph().nodes.len(),
            nodes_before,
            "a refused spawn leaves the graph as it was"
        );

        let mut over_a_run = Graph::empty();
        let run_node = Uuid::from_u128(0x72);
        over_a_run.root = Some(run_node);
        over_a_run.nodes.insert(
            run_node,
            GraphNode {
                id: run_node,
                function: nodes::SUCCEED_FUNCTION_ID,
                ..GraphNode::default()
            },
        );
        assert!(
            interp.load(over_a_run).is_err(),
            "a load cannot replace a run's node"
        );
    }

    /// A run's variables leave with it: once halted, a graph may declare the
    /// same variable under another name. While it runs, a load cannot rename a
    /// variable the run reads.
    #[test]
    fn a_run_s_variables_leave_with_it() {
        let leaf = Uuid::from_u128(0xE1);
        let (mut bridge, _statuses, _ticks) = scripted(leaf, Status::Success);
        let store = SimpleDataStore::new();
        let mut interp = BehaviorTreeInterpreter::new();
        let variable = Uuid::from_u128(0x6);
        let handle = interp
            .spawn_graph(
                &graph_type(),
                reading(0xB0, variable, "a"),
                RunPolicy::Concurrent,
            )
            .unwrap();
        assert!(
            interp.load(reading(0xB1, variable, "b")).is_err(),
            "a load cannot rename a variable a run reads"
        );

        interp.halt(handle.id).unwrap();
        let mut ctx = BehaviorContext {
            functions: &arora_behavior::FunctionIndex::new(),
            store: &store,
            call_bridge: &mut bridge,
        };
        interp.tick(&mut ctx).unwrap();
        assert!(!interp.graph().variables.contains_key(&variable));
        interp
            .spawn_graph(
                &graph_type(),
                reading(0xB2, variable, "b"),
                RunPolicy::Concurrent,
            )
            .expect("the halted run's declaration is gone");
    }

    /// A run whose program errors ends `Failure`: the error is reported once,
    /// and the other runs tick on.
    #[test]
    fn a_run_that_errors_ends_failure_and_the_others_tick_on() {
        let good = Uuid::from_u128(0xF5);
        let missing = Uuid::from_u128(0xF6);
        let (mut bridge, _statuses, ticks) = scripted(good, Status::Success);
        let store = SimpleDataStore::new();
        let mut interp = BehaviorTreeInterpreter::new();
        let failing = interp
            .spawn_graph(
                &graph_type(),
                stepping_graph([0x10, 0x11, 0x12], missing),
                RunPolicy::Concurrent,
            )
            .unwrap();
        interp
            .spawn_graph(
                &graph_type(),
                stepping_graph([0x20, 0x21, 0x22], good),
                RunPolicy::Concurrent,
            )
            .unwrap();
        let mut ctx = BehaviorContext {
            functions: &arora_behavior::FunctionIndex::new(),
            store: &store,
            call_bridge: &mut bridge,
        };
        assert!(interp.tick(&mut ctx).is_err(), "the error is reported");
        let failure: Value = Status::Failure.into();
        assert_eq!(
            store.read(std::slice::from_ref(&failing.status)),
            vec![Some(failure)]
        );
        for _ in 0..2 {
            interp
                .tick(&mut ctx)
                .expect("the failed run no longer errors");
        }
        assert_eq!(
            ticks.borrow().get(&good),
            Some(&2),
            "the other run ticks on"
        );
    }

    /// A finished or halted run's node ids are free again at once, so a client
    /// replaces a program by halting it and spawning the new one before the
    /// next step.
    #[test]
    fn a_halted_or_finished_run_s_ids_can_be_spawned_again() {
        let leaf = Uuid::from_u128(0xF7);
        let (mut bridge, _statuses, _ticks) = scripted(leaf, Status::Success);
        let store = SimpleDataStore::new();
        let mut interp = BehaviorTreeInterpreter::new();
        let ids = [0x30, 0x31, 0x32];
        let first = interp
            .spawn_graph(
                &graph_type(),
                stepping_graph(ids, leaf),
                RunPolicy::Concurrent,
            )
            .unwrap();
        interp.halt(first.id).unwrap();
        let second = interp
            .spawn_graph(
                &graph_type(),
                stepping_graph(ids, leaf),
                RunPolicy::Concurrent,
            )
            .expect("the halted run's ids are free");
        let mut ctx = BehaviorContext {
            functions: &arora_behavior::FunctionIndex::new(),
            store: &store,
            call_bridge: &mut bridge,
        };
        interp.tick(&mut ctx).unwrap();
        let failure: Value = Status::Failure.into();
        let running: Value = Status::Running.into();
        assert_eq!(
            store.read(&[first.status.clone(), second.status.clone()]),
            vec![Some(failure), Some(running)]
        );

        // A run that ends by itself: a sequence of one succeeding leaf.
        use arora_behavior::graph::{Graph, Node as GraphNode};
        let one = Uuid::from_u128(0x40);
        let mut once = Graph::empty();
        once.root = Some(one);
        once.nodes.insert(
            one,
            GraphNode {
                id: one,
                function: nodes::SUCCEED_FUNCTION_ID,
                ..GraphNode::default()
            },
        );
        interp
            .spawn_graph(&graph_type(), once.clone(), RunPolicy::Concurrent)
            .unwrap();
        interp.tick(&mut ctx).unwrap();
        interp
            .spawn_graph(&graph_type(), once, RunPolicy::Concurrent)
            .expect("the finished run's ids are free");
    }

    /// A `seq_star` (`ids[0]`) over two run-call leaves (`ids[1]`, `ids[2]`)
    /// calling `leaves`. With the first succeeding and the second running, it
    /// stands at its second: calling the first again means it restarted.
    fn seq_star_graph(ids: [u128; 3], leaves: [Uuid; 2]) -> arora_behavior::graph::Graph {
        use arora_behavior::graph::{Graph, Io, Link, LinkSource, Node as GraphNode, Port};
        let [root, first, second] = ids.map(Uuid::from_u128);
        let mut graph = Graph::empty();
        graph.root = Some(root);
        graph.nodes.insert(
            root,
            GraphNode {
                id: root,
                function: nodes::SEQ_STAR_FUNCTION_ID,
                children: Some(vec![first, second]),
                ..GraphNode::default()
            },
        );
        graph.links.push(Link::new(
            Port::new(root, nodes::SEQ_STAR_CURRENT_INDEX_PARAM_ID),
            LinkSource::Literal(Value::U16(0)),
        ));
        for (node, leaf) in [(first, leaves[0]), (second, leaves[1])] {
            graph.nodes.insert(
                node,
                GraphNode {
                    id: node,
                    function: nodes::RUN_CALL_FUNCTION_ID,
                    inputs: vec![Io::new(nodes::RUN_CALL_PARAM_ID)],
                    ..GraphNode::default()
                },
            );
            graph.links.push(Link::new(
                Port::new(node, nodes::RUN_CALL_PARAM_ID),
                LinkSource::Literal(
                    arora_types::value_serde::to_value(&call_to(0xB0, leaf)).unwrap(),
                ),
            ));
        }
        graph
    }

    /// A graph spawned again as a live run runs it answers that run's handle
    /// and changes nothing, its variables included: the run goes on where it
    /// stood.
    #[test]
    fn a_graph_spawned_again_as_it_runs_answers_its_run() {
        let [a1, a2] = [0x91, 0x92].map(Uuid::from_u128);
        let statuses: LeafStatuses = Rc::new(RefCell::new(HashMap::from([
            (a1, Status::Success),
            (a2, Status::Running),
        ])));
        let ticks: LeafTicks = Rc::new(RefCell::new(HashMap::new()));
        let mut bridge = TestBridge::new(statuses, ticks.clone());
        let store = SimpleDataStore::new();
        let mut interp = BehaviorTreeInterpreter::new();
        let declaring = |mut graph: arora_behavior::graph::Graph| {
            graph
                .variables
                .insert(Uuid::from_u128(0x7A), "face/w".to_string());
            graph
        };
        let spawned = interp
            .spawn_graph(
                &graph_type(),
                declaring(seq_star_graph([0x90, 0x93, 0x94], [a1, a2])),
                RunPolicy::Concurrent,
            )
            .unwrap();
        let mut ctx = BehaviorContext {
            functions: &arora_behavior::FunctionIndex::new(),
            store: &store,
            call_bridge: &mut bridge,
        };
        interp.tick(&mut ctx).unwrap();

        let before = interp.graph().clone();
        let again = interp
            .spawn_graph(
                &graph_type(),
                declaring(seq_star_graph([0x90, 0x93, 0x94], [a1, a2])),
                RunPolicy::Concurrent,
            )
            .expect("the run's graph spawned again is answered");
        assert_eq!(again, spawned, "the answer is the run's handle");
        assert_eq!(interp.graph(), &before, "the graph it runs is unchanged");
        interp.tick(&mut ctx).unwrap();
        assert_eq!(ticks.borrow().get(&a1), Some(&1), "the run did not restart");
        assert_eq!(ticks.borrow().get(&a2), Some(&2), "the run goes on");
    }

    /// A graph spawned with a live run's root and other nodes or links is that
    /// run's program from then on: the nodes and links that differ change,
    /// the nodes it drops leave but for one the main behavior holds, and the
    /// handle answered halts it.
    #[test]
    fn a_graph_spawned_again_with_changes_becomes_its_run_s_program() {
        use arora_behavior::graph::{GraphDiff, Link, LinkSource, Node as GraphNode, Port};
        let [a, b] = [0xA1, 0xB1].map(Uuid::from_u128);
        let statuses: LeafStatuses = Rc::new(RefCell::new(HashMap::from([
            (a, Status::Running),
            (b, Status::Running),
        ])));
        let ticks: LeafTicks = Rc::new(RefCell::new(HashMap::new()));
        let mut bridge = TestBridge::new(statuses, ticks.clone());
        let store = SimpleDataStore::new();
        let mut interp = BehaviorTreeInterpreter::new();
        let spawned = interp
            .spawn_graph(
                &graph_type(),
                stepping_graph([0xC0, 0xC1, 0xC2], a),
                RunPolicy::Concurrent,
            )
            .unwrap();
        let mut ctx = BehaviorContext {
            functions: &arora_behavior::FunctionIndex::new(),
            store: &store,
            call_bridge: &mut bridge,
        };
        interp.tick(&mut ctx).unwrap();

        // An edit puts the main behavior's root under the run too, and links
        // a port of the run's root.
        let [parallel, call_node, run, added] = [0xC0, 0xC1, 0xC2, 0xC3].map(Uuid::from_u128);
        let main = Uuid::from_u128(0xA0);
        interp
            .load(reading(0xA0, Uuid::from_u128(0x8), "face/x"))
            .unwrap();
        let extra = Port::new(parallel, Uuid::from_u128(0x5));
        interp
            .apply(GraphDiff {
                add_nodes: vec![GraphNode {
                    id: parallel,
                    function: nodes::PARALLEL_FUNCTION_ID,
                    children: Some(vec![call_node, run, main]),
                    ..GraphNode::default()
                }],
                add_links: vec![Link::new(extra, LinkSource::Literal(Value::Unit))],
                ..GraphDiff::default()
            })
            .unwrap();

        // The call leaf calls `b`, and a new `Run` node replaces the old one.
        let mut changed = stepping_graph([0xC0, 0xC1, 0xC2], b);
        changed.nodes.remove(&run);
        changed.nodes.insert(
            added,
            GraphNode {
                id: added,
                function: nodes::RUN_FUNCTION_ID,
                ..GraphNode::default()
            },
        );
        changed.nodes.get_mut(&parallel).unwrap().children = Some(vec![call_node, added]);
        let declared = Uuid::from_u128(0x7B);
        changed.variables.insert(declared, "face/z".to_string());
        changed.links.push(Link::new(
            Port::new(added, Uuid::from_u128(0x6)),
            LinkSource::Variable(declared),
        ));
        let again = interp
            .spawn_graph(&graph_type(), changed, RunPolicy::Concurrent)
            .expect("a graph with a live run's root is answered");
        assert_eq!(again, spawned, "the answer is the run's handle");
        let graph = interp.graph();
        assert!(
            graph.nodes.contains_key(&added),
            "the new node joined the run"
        );
        assert!(!graph.nodes.contains_key(&run), "the dropped node left");
        assert!(
            graph.nodes.contains_key(&main),
            "a dropped node the main behavior holds stays"
        );
        assert_eq!(graph.link_to(&extra), None, "a link the graph lacks left");
        assert_eq!(
            graph.link_to(&Port::new(call_node, nodes::RUN_CALL_PARAM_ID)),
            Some(&Link::new(
                Port::new(call_node, nodes::RUN_CALL_PARAM_ID),
                LinkSource::Literal(arora_types::value_serde::to_value(&call_to(0xB0, b)).unwrap()),
            )),
            "the call leaf's link is the new graph's"
        );
        interp.tick(&mut ctx).unwrap();
        assert_eq!(ticks.borrow().get(&a), Some(&1), "the old call is gone");
        assert_eq!(
            ticks.borrow().get(&b),
            Some(&1),
            "the run calls the new leaf"
        );

        interp.halt(again.id).unwrap();
        interp.tick(&mut ctx).unwrap();
        let failure: Value = Status::Failure.into();
        assert_eq!(
            store.read(std::slice::from_ref(&spawned.status)),
            vec![Some(failure)],
            "the handle answered halts the run"
        );
        assert_eq!(
            interp.graph().nodes.len(),
            2,
            "the runner and the main behavior remain"
        );
        assert!(
            !interp.graph().variables.contains_key(&declared),
            "a variable the run's new graph declared leaves with it"
        );
    }

    /// A graph with a live run's root is refused, changing nothing, when it
    /// holds a node of the main behavior or of another run — one an edit
    /// placed under the run too — the run's decorator, or a variable in use
    /// renamed.
    #[test]
    fn a_graph_with_a_live_run_s_root_and_others_nodes_is_refused() {
        use arora_behavior::graph::{GraphDiff, Node as GraphNode};
        let leaf = Uuid::from_u128(0xD1);
        let mut interp = BehaviorTreeInterpreter::new();
        interp
            .spawn_graph(
                &graph_type(),
                stepping_graph([0xE0, 0xE1, 0xE2], leaf),
                RunPolicy::Concurrent,
            )
            .unwrap();
        interp
            .spawn_graph(
                &graph_type(),
                stepping_graph([0xF0, 0xF1, 0xF2], leaf),
                RunPolicy::Concurrent,
            )
            .unwrap();
        let variable = Uuid::from_u128(0x7);
        interp.load(reading(0xA0, variable, "face/x")).unwrap();
        let before = interp.graph().clone();
        let decorator = before.nodes[&before.root.unwrap()]
            .children
            .as_ref()
            .unwrap()[1];

        let [parallel, call_node] = [0xE0, 0xE1].map(Uuid::from_u128);
        let holding = |other: Uuid| {
            let mut graph = stepping_graph([0xE0, 0xE1, 0xE2], leaf);
            graph.nodes.insert(
                other,
                GraphNode {
                    id: other,
                    function: nodes::RUN_FUNCTION_ID,
                    ..GraphNode::default()
                },
            );
            graph.nodes.get_mut(&parallel).unwrap().children =
                Some(vec![call_node, Uuid::from_u128(0xE2), other]);
            graph
        };
        let mut renaming = stepping_graph([0xE0, 0xE1, 0xE2], leaf);
        renaming.variables.insert(variable, "face/y".to_string());
        for (what, graph) in [
            (
                "a node of the main behavior",
                holding(Uuid::from_u128(0xA0)),
            ),
            ("a node of another run", holding(Uuid::from_u128(0xF2))),
            ("the run's decorator", holding(decorator)),
            ("a variable in use renamed", renaming),
        ] {
            assert!(
                interp
                    .spawn_graph(&graph_type(), graph, RunPolicy::Concurrent)
                    .is_err(),
                "a graph with {what} is refused"
            );
            assert_eq!(interp.graph(), &before, "{what}: the graph is unchanged");
        }

        // An edit places the main behavior's root and the other run's root
        // under the run: the run holds them, but not as its own.
        let [main, other_root, run] = [0xA0, 0xF0, 0xE2].map(Uuid::from_u128);
        interp
            .apply(GraphDiff {
                add_nodes: vec![GraphNode {
                    id: parallel,
                    function: nodes::PARALLEL_FUNCTION_ID,
                    children: Some(vec![call_node, run, main, other_root]),
                    ..GraphNode::default()
                }],
                ..GraphDiff::default()
            })
            .unwrap();
        let before = interp.graph().clone();
        for (what, shared) in [
            ("the main behavior's root", main),
            ("another run's root", other_root),
        ] {
            let mut graph = stepping_graph([0xE0, 0xE1, 0xE2], leaf);
            graph.nodes.insert(
                shared,
                GraphNode {
                    id: shared,
                    function: nodes::RUN_FUNCTION_ID,
                    ..GraphNode::default()
                },
            );
            graph.nodes.get_mut(&parallel).unwrap().children = Some(vec![call_node, run, shared]);
            assert!(
                interp
                    .spawn_graph(&graph_type(), graph, RunPolicy::Concurrent)
                    .is_err(),
                "a graph redefining {what}, which the run shares, is refused"
            );
            assert_eq!(interp.graph(), &before, "{what}: the graph is unchanged");
        }
    }

    /// A halt keeps the nodes the main behavior also holds, its root
    /// included: an edit may place the main behavior's root under a run.
    #[test]
    fn a_halt_keeps_the_main_behavior_s_root_a_run_also_holds() {
        use arora_behavior::graph::{GraphDiff, Node as GraphNode};
        let leaf = Uuid::from_u128(0xD2);
        let mut interp = BehaviorTreeInterpreter::new();
        let handle = interp
            .spawn_graph(
                &graph_type(),
                stepping_graph([0x60, 0x61, 0x62], leaf),
                RunPolicy::Concurrent,
            )
            .unwrap();
        let main = Uuid::from_u128(0xA0);
        interp
            .load(reading(0xA0, Uuid::from_u128(0x9), "face/x"))
            .unwrap();
        let [parallel, call_node, run] = [0x60, 0x61, 0x62].map(Uuid::from_u128);
        interp
            .apply(GraphDiff {
                add_nodes: vec![GraphNode {
                    id: parallel,
                    function: nodes::PARALLEL_FUNCTION_ID,
                    children: Some(vec![call_node, run, main]),
                    ..GraphNode::default()
                }],
                ..GraphDiff::default()
            })
            .unwrap();

        interp.halt(handle.id).expect("the halt applies");
        let graph = interp.graph();
        assert!(graph.nodes.contains_key(&main), "the main behavior stays");
        assert_eq!(
            graph.nodes.len(),
            2,
            "the runner and the main behavior remain"
        );
    }

    /// An edit cannot place the runner or a decorator elsewhere, call a node
    /// that is not there, or close a cycle; a refused edit changes nothing.
    #[test]
    fn an_edit_that_would_break_the_tree_is_refused() {
        use arora_behavior::graph::{GraphDiff, Node as GraphNode};

        let leaf = Uuid::from_u128(0xF8);
        let mut interp = BehaviorTreeInterpreter::new();
        interp
            .spawn_graph(
                &graph_type(),
                stepping_graph([0x50, 0x51, 0x52], leaf),
                RunPolicy::Concurrent,
            )
            .unwrap();
        let runner = interp.graph().root.unwrap();
        let decorator = interp.graph().nodes[&runner].children.as_ref().unwrap()[0];
        let [parallel, _, run] = [0x50, 0x51, 0x52].map(Uuid::from_u128);
        let before = interp.graph().clone();
        let node = |id, children| GraphNode {
            id,
            function: nodes::SEQ_FUNCTION_ID,
            children: Some(children),
            ..GraphNode::default()
        };
        for (what, diff) in [
            (
                "the main root set to a decorator",
                GraphDiff {
                    set_root: Some(decorator),
                    ..GraphDiff::default()
                },
            ),
            (
                "a decorator as a child",
                GraphDiff {
                    add_nodes: vec![node(Uuid::from_u128(0x60), vec![decorator])],
                    ..GraphDiff::default()
                },
            ),
            (
                "the runner as a child",
                GraphDiff {
                    add_nodes: vec![node(Uuid::from_u128(0x60), vec![runner])],
                    ..GraphDiff::default()
                },
            ),
            (
                "a run's node removed under its parent",
                GraphDiff {
                    remove_nodes: vec![run],
                    ..GraphDiff::default()
                },
            ),
            (
                "a cycle",
                GraphDiff {
                    add_nodes: vec![node(run, vec![parallel])],
                    ..GraphDiff::default()
                },
            ),
        ] {
            assert!(interp.apply(diff).is_err(), "{what} is refused");
            assert_eq!(interp.graph(), &before, "{what} changed nothing");
        }
    }
}

/// What a change of the graph keeps: the state of every node it leaves in
/// place. A `seq_star` whose first child succeeded and whose second runs
/// stands at its second child; calling its first child again means it
/// restarted.
mod kept_state {
    use super::{LeafStatuses, LeafTicks, TestBridge};
    use crate::behavior::{graph_type, BehaviorTreeInterpreter};
    use crate::nodes;
    use arora_behavior::graph::{Graph, GraphDiff, Io, Link, LinkSource, Node as GraphNode, Port};
    use arora_behavior::Status;
    use arora_behavior::{
        BehaviorContext, BehaviorError, BehaviorInterpreter, FunctionIndex, RunPolicy,
    };
    use arora_simple_data_store::SimpleDataStore;
    use arora_types::call::Call;
    use arora_types::data::DataStore;
    use arora_types::gen_uuid_from_str;
    use arora_types::value::Value;
    use std::cell::RefCell;
    use std::rc::Rc;
    use uuid::Uuid;

    /// An interpreter, what its ticks need, and the scripted leaves'
    /// statuses and calls.
    struct Device {
        interp: BehaviorTreeInterpreter,
        bridge: TestBridge,
        store: SimpleDataStore,
        statuses: LeafStatuses,
        ticks: LeafTicks,
        functions: FunctionIndex,
    }

    impl Device {
        /// A device whose scripted leaves answer `statuses`.
        fn new(statuses: &[(Uuid, Status)]) -> Self {
            let statuses: LeafStatuses = Rc::new(RefCell::new(statuses.iter().cloned().collect()));
            let ticks = LeafTicks::default();
            Device {
                interp: BehaviorTreeInterpreter::new(),
                bridge: TestBridge::new(statuses.clone(), ticks.clone()),
                store: SimpleDataStore::new(),
                statuses,
                ticks,
                functions: FunctionIndex::new(),
            }
        }

        fn try_tick(&mut self) -> Result<(), BehaviorError> {
            let mut ctx = BehaviorContext {
                functions: &self.functions,
                store: &self.store,
                call_bridge: &mut self.bridge,
            };
            self.interp.tick(&mut ctx).map(|_| ())
        }

        fn tick(&mut self) {
            self.try_tick().expect("the tick succeeds");
        }

        /// How many times the scripted leaf `leaf` was called.
        fn calls(&self, leaf: Uuid) -> u32 {
            self.ticks.borrow().get(&leaf).copied().unwrap_or(0)
        }
    }

    /// The link that makes the run-call leaf `node` call the scripted leaf
    /// `leaf`.
    fn call_link(node: Uuid, leaf: Uuid) -> Link {
        let call = Call {
            module_id: Some(Uuid::from_u128(0xB0)),
            id: leaf,
            args: Vec::new(),
        };
        Link::new(
            Port::new(node, nodes::RUN_CALL_PARAM_ID),
            LinkSource::Literal(arora_types::value_serde::to_value(&call).unwrap()),
        )
    }

    /// Add to `graph` the run-call leaf `node`, calling `leaf`.
    fn add_call(graph: &mut Graph, node: Uuid, leaf: Uuid) {
        graph
            .nodes
            .insert(node, leaf_node(node, nodes::RUN_CALL_FUNCTION_ID));
        graph.links.push(call_link(node, leaf));
    }

    fn leaf_node(id: Uuid, function: Uuid) -> GraphNode {
        GraphNode {
            id,
            function,
            ..GraphNode::default()
        }
    }

    fn control(id: Uuid, function: Uuid, children: Vec<Uuid>) -> GraphNode {
        GraphNode {
            id,
            function,
            children: Some(children),
            ..GraphNode::default()
        }
    }

    /// The link that starts the `seq_star` `node` at its first child.
    fn first_child(node: Uuid) -> Link {
        Link::new(
            Port::new(node, nodes::SEQ_STAR_CURRENT_INDEX_PARAM_ID),
            LinkSource::Literal(Value::U16(0)),
        )
    }

    /// A `seq_star` (`ids[0]`) over two run-call leaves (`ids[1]`, `ids[2]`)
    /// calling `leaves`.
    fn seq_star(ids: [u128; 3], leaves: [Uuid; 2]) -> Graph {
        let [root, first, second] = ids.map(Uuid::from_u128);
        let mut graph = Graph::empty();
        graph.root = Some(root);
        graph.nodes.insert(
            root,
            control(root, nodes::SEQ_STAR_FUNCTION_ID, vec![first, second]),
        );
        graph.links.push(first_child(root));
        add_call(&mut graph, first, leaves[0]);
        add_call(&mut graph, second, leaves[1]);
        graph
    }

    /// A sequence (`ids[0]`) over one run-call leaf (`ids[1]`) calling
    /// `leaf`: it ends when the leaf does.
    fn sequence(ids: [u128; 2], leaf: Uuid) -> Graph {
        let [root, call] = ids.map(Uuid::from_u128);
        let mut graph = Graph::empty();
        graph.root = Some(root);
        graph
            .nodes
            .insert(root, control(root, nodes::SEQ_FUNCTION_ID, vec![call]));
        add_call(&mut graph, call, leaf);
        graph
    }

    /// Spawning a run, editing it, halting it, and another run ending, leave
    /// the main behavior's `seq_star` and another run's where they stand.
    #[test]
    fn a_run_s_changes_leave_the_other_behaviors_where_they_stand() {
        let [m1, m2, a1, a2, b1, b2] = [0xA1, 0xA2, 0xB1, 0xB2, 0xC1, 0xC2].map(Uuid::from_u128);
        let mut device = Device::new(&[
            (m1, Status::Success),
            (m2, Status::Running),
            (a1, Status::Success),
            (a2, Status::Running),
            (b1, Status::Running),
            (b2, Status::Running),
        ]);
        device
            .interp
            .load(seq_star([0x10, 0x11, 0x12], [m1, m2]))
            .unwrap();
        device
            .interp
            .spawn_graph(
                &graph_type(),
                seq_star([0x20, 0x21, 0x22], [a1, a2]),
                RunPolicy::Concurrent,
            )
            .unwrap();
        device.tick();
        let mut ticks = 1;
        // Each tick resumes both at their second child.
        let mut tick = |device: &mut Device, after: &str| {
            device.tick();
            ticks += 1;
            for (first, second) in [(m1, m2), (a1, a2)] {
                assert_eq!(
                    device.calls(first),
                    1,
                    "{after}, {first}'s seq_star restarted"
                );
                assert_eq!(device.calls(second), ticks, "{after}");
            }
        };

        let b = device
            .interp
            .spawn_graph(
                &graph_type(),
                sequence([0x30, 0x31], b1),
                RunPolicy::Concurrent,
            )
            .unwrap();
        tick(&mut device, "after a spawn");
        device
            .interp
            .apply(GraphDiff {
                add_links: vec![call_link(Uuid::from_u128(0x31), b2)],
                ..GraphDiff::default()
            })
            .unwrap();
        tick(&mut device, "after an edit");
        assert_eq!(device.calls(b2), 1, "the edited run calls its new leaf");
        device.interp.halt(b.id).unwrap();
        tick(&mut device, "after a halt");

        let b = device
            .interp
            .spawn_graph(
                &graph_type(),
                sequence([0x30, 0x31], b1),
                RunPolicy::Concurrent,
            )
            .unwrap();
        device.statuses.borrow_mut().insert(b1, Status::Success);
        tick(&mut device, "after a run ended");
        tick(&mut device, "after the ended run left");
        let success: Value = Status::Success.into();
        assert_eq!(
            device.store.read(std::slice::from_ref(&b.status)),
            vec![Some(success)]
        );
        assert_eq!(
            device.interp.graph().nodes.len(),
            1 + 3 + 4,
            "the ended run left"
        );
    }

    /// An edit restarts the nodes it replaces or relinks, and only them.
    #[test]
    fn an_edit_restarts_the_nodes_it_touches() {
        let [a1, a2, a3] = [0xA1, 0xA2, 0xA3].map(Uuid::from_u128);
        let mut device = Device::new(&[
            (a1, Status::Success),
            (a2, Status::Running),
            (a3, Status::Running),
        ]);
        let [root, first, second] = [0x20, 0x21, 0x22].map(Uuid::from_u128);
        device
            .interp
            .spawn_graph(
                &graph_type(),
                seq_star([0x20, 0x21, 0x22], [a1, a2]),
                RunPolicy::Concurrent,
            )
            .unwrap();
        device.tick();

        device
            .interp
            .apply(GraphDiff {
                add_links: vec![call_link(second, a3)],
                ..GraphDiff::default()
            })
            .unwrap();
        device.tick();
        assert_eq!(device.calls(a1), 1, "a relinked child leaves its seq_star");
        assert_eq!(device.calls(a3), 1, "the relinked child calls its new leaf");

        device
            .interp
            .apply(GraphDiff {
                add_nodes: vec![control(
                    root,
                    nodes::SEQ_STAR_FUNCTION_ID,
                    vec![first, second],
                )],
                ..GraphDiff::default()
            })
            .unwrap();
        device.tick();
        assert_eq!(device.calls(a1), 2, "a replaced seq_star restarts");

        device
            .interp
            .apply(GraphDiff {
                add_links: vec![first_child(root)],
                ..GraphDiff::default()
            })
            .unwrap();
        device.tick();
        assert_eq!(device.calls(a1), 3, "a relinked seq_star restarts");
    }

    /// The nodes of a `once` behavior.
    const ONCE: [u128; 5] = [0x10, 0x11, 0x12, 0x13, 0x14];

    /// `Fallback(IsStringSet(.done), Sequence(call m, SetString(.done)))` on
    /// nodes `ids`: it calls `m` once, then `.done` is set.
    fn once(m: Uuid, ids: [u128; 5]) -> Graph {
        let [root, done, first, call, set] = ids.map(Uuid::from_u128);
        let variable = gen_uuid_from_str(".done");
        let mut graph = Graph::empty();
        graph.root = Some(root);
        graph.nodes.insert(
            root,
            control(root, nodes::FALLBACK_FUNCTION_ID, vec![done, first]),
        );
        graph.nodes.insert(
            first,
            control(first, nodes::SEQ_FUNCTION_ID, vec![call, set]),
        );
        graph
            .nodes
            .insert(done, leaf_node(done, nodes::IS_STR_SET_FUNCTION_ID));
        graph
            .nodes
            .insert(set, leaf_node(set, nodes::SET_STR_FUNCTION_ID));
        add_call(&mut graph, call, m);
        graph.links.extend([
            Link::new(
                Port::new(done, nodes::IS_STR_SET_VALUE_PARAM_ID),
                LinkSource::Variable(variable),
            ),
            Link::new(
                Port::new(set, nodes::SET_STR_VARIABLE_PARAM_ID),
                LinkSource::Variable(variable),
            ),
            Link::new(
                Port::new(set, nodes::SET_STR_VALUE_PARAM_ID),
                LinkSource::Literal(Value::String("yes".to_string())),
            ),
        ]);
        graph.variables.insert(variable, ".done".to_string());
        graph
    }

    /// A tree-local variable keeps its value while a node the change left in
    /// place reads it, and starts afresh with the nodes that read it.
    #[test]
    fn a_tree_local_variable_lives_as_long_as_a_node_reading_it() {
        let [m, r] = [0xA1, 0xB1].map(Uuid::from_u128);
        let mut device = Device::new(&[(m, Status::Success), (r, Status::Running)]);
        device.interp.load(once(m, ONCE)).unwrap();
        device.tick();
        device.tick();
        assert_eq!(device.calls(m), 1);

        let run = device
            .interp
            .spawn_graph(
                &graph_type(),
                sequence([0x30, 0x31], r),
                RunPolicy::Concurrent,
            )
            .unwrap();
        device.tick();
        device.interp.halt(run.id).unwrap();
        device.tick();
        assert_eq!(device.calls(m), 1, "a spawn and a halt kept .done");

        device.interp.load(once(m, ONCE)).unwrap();
        device.tick();
        assert_eq!(
            device.calls(m),
            2,
            "a load restarts the behavior, .done with it"
        );
    }

    /// A load keeps a tree-local variable a run also reads: the run, which the
    /// load leaves in place, sees it unchanged.
    #[test]
    fn a_load_keeps_a_tree_local_variable_a_run_reads() {
        let [m, r] = [0xA1, 0xB1].map(Uuid::from_u128);
        let mut device = Device::new(&[(m, Status::Success), (r, Status::Running)]);
        device.interp.load(once(m, ONCE)).unwrap();
        // The run calls `r`, then would check `.done`.
        let [root, call, done] = [0x30, 0x31, 0x32].map(Uuid::from_u128);
        let variable = gen_uuid_from_str(".done");
        let mut run = Graph::empty();
        run.root = Some(root);
        run.nodes.insert(
            root,
            control(root, nodes::SEQ_FUNCTION_ID, vec![call, done]),
        );
        run.nodes
            .insert(done, leaf_node(done, nodes::IS_STR_SET_FUNCTION_ID));
        add_call(&mut run, call, r);
        run.links.push(Link::new(
            Port::new(done, nodes::IS_STR_SET_VALUE_PARAM_ID),
            LinkSource::Variable(variable),
        ));
        run.variables.insert(variable, ".done".to_string());
        device
            .interp
            .spawn_graph(&graph_type(), run, RunPolicy::Concurrent)
            .unwrap();
        device.tick();
        device.tick();
        assert_eq!(device.calls(m), 1);

        device
            .interp
            .load(once(m, [0x40, 0x41, 0x42, 0x43, 0x44]))
            .unwrap();
        device.tick();
        assert_eq!(device.calls(m), 1, "the run's .done kept its value");
    }

    /// Unlinking a node's input, or setting the predetermined key of one,
    /// restarts the node.
    #[test]
    fn unlinking_or_rekeying_a_node_restarts_it() {
        let [a1, a2] = [0xA1, 0xA2].map(Uuid::from_u128);
        let mut device = Device::new(&[(a1, Status::Success), (a2, Status::Running)]);
        let root = Uuid::from_u128(0x20);
        let extra = Port::new(root, Uuid::from_u128(0xE0));
        let mut graph = seq_star([0x20, 0x21, 0x22], [a1, a2]);
        graph.nodes.get_mut(&root).unwrap().inputs = vec![Io::new(extra.port)];
        graph
            .links
            .push(Link::new(extra, LinkSource::Literal(Value::U8(7))));
        device.interp.load(graph).unwrap();
        device.tick();

        device
            .interp
            .apply(GraphDiff {
                remove_links: vec![extra],
                ..GraphDiff::default()
            })
            .unwrap();
        device.tick();
        assert_eq!(device.calls(a1), 2, "an unlinked seq_star restarts");

        device
            .interp
            .apply(GraphDiff {
                set_predetermined: vec![(extra, Some(".k".to_string()))],
                ..GraphDiff::default()
            })
            .unwrap();
        device.tick();
        assert_eq!(device.calls(a1), 3, "a re-keyed seq_star restarts");
    }

    /// A refused edit restarts nothing, at the next lowering either.
    #[test]
    fn a_refused_edit_restarts_nothing() {
        let [a1, a2, r] = [0xA1, 0xA2, 0xB1].map(Uuid::from_u128);
        let mut device = Device::new(&[
            (a1, Status::Success),
            (a2, Status::Running),
            (r, Status::Running),
        ]);
        let [root, first, second] = [0x20, 0x21, 0x22].map(Uuid::from_u128);
        device
            .interp
            .load(seq_star([0x20, 0x21, 0x22], [a1, a2]))
            .unwrap();
        device.tick();

        // It relinks the seq_star's index, and names a child that is not a
        // node: refused whole.
        let refused = device.interp.apply(GraphDiff {
            add_nodes: vec![control(
                root,
                nodes::SEQ_STAR_FUNCTION_ID,
                vec![first, second, Uuid::from_u128(0xBAD)],
            )],
            add_links: vec![first_child(root)],
            ..GraphDiff::default()
        });
        assert!(refused.is_err());
        device
            .interp
            .spawn_graph(
                &graph_type(),
                sequence([0x30, 0x31], r),
                RunPolicy::Concurrent,
            )
            .unwrap();
        device.tick();
        assert_eq!(device.calls(a1), 1, "the seq_star kept its place");
    }

    /// A graph that does not lower leaves the previous lowering in place, and
    /// the next lowering that succeeds takes over its state.
    #[test]
    fn a_graph_that_does_not_lower_keeps_its_state_for_the_next_lowering() {
        use crate::ModuleFunction;
        use arora_types::data::Key;
        use arora_types::record::module::frozen::{Function, Parameter};
        use arora_types::record::ty::{FrozenTy, PrimitiveKind};
        use std::collections::HashMap;

        let [a1, a2] = [0xA1, 0xA2].map(Uuid::from_u128);
        let mut device = Device::new(&[(a1, Status::Success), (a2, Status::Running)]);
        let [function, frame] = [0xF00, 0xF01].map(Uuid::from_u128);
        device.functions = HashMap::from([(
            function,
            ModuleFunction {
                module_id: Uuid::from_u128(0xF05),
                function_id: function,
                function_name: "consume".to_string(),
                function: Function {
                    parameters: HashMap::from([(
                        frame,
                        Parameter {
                            name: "frame".to_string(),
                            ty: FrozenTy::from(PrimitiveKind::U64),
                            mutable: true,
                        },
                    )]),
                    parameter_ordering: vec![frame],
                    return_ty: FrozenTy::from(PrimitiveKind::U8),
                },
            },
        )]);
        device
            .interp
            .load(seq_star([0x20, 0x21, 0x22], [a1, a2]))
            .unwrap();
        device.tick();

        // A selection onto a parameter the index declares mutable: the edit
        // applies, and the lowering refuses it.
        let consumer = Uuid::from_u128(0x30);
        device
            .interp
            .apply(GraphDiff {
                add_nodes: vec![GraphNode {
                    id: consumer,
                    function,
                    inputs: vec![Io::new(frame)],
                    ..GraphNode::default()
                }],
                add_links: vec![Link::new(
                    Port::new(consumer, frame),
                    LinkSource::Select {
                        source: Box::new(LinkSource::Variable(Uuid::from_u128(0x31))),
                        path: Key::new(".a"),
                    },
                )],
                ..GraphDiff::default()
            })
            .unwrap();
        assert!(device.try_tick().is_err(), "the graph does not lower");
        device
            .interp
            .apply(GraphDiff {
                remove_nodes: vec![consumer],
                ..GraphDiff::default()
            })
            .unwrap();
        device.tick();
        assert_eq!(
            (device.calls(a1), device.calls(a2)),
            (1, 2),
            "the seq_star kept its place across the failed lowering"
        );
    }

    /// A tree-local variable that a node left in place reads through a
    /// selection keeps its value.
    #[test]
    fn a_variable_read_through_a_selection_keeps_its_value() {
        use arora_types::data::Key;
        use arora_types::value::{Structure, StructureField};

        let m = Uuid::from_u128(0xA1);
        let mut device = Device::new(&[(m, Status::Success)]);
        let [root, equal, first, call, unpack] =
            [0x10, 0x11, 0x12, 0x13, 0x14].map(Uuid::from_u128);
        let x = gen_uuid_from_str(".x");
        let field = Uuid::from_u128(0xF1E1D);
        let values = Link::new(
            Port::new(unpack, nodes::UNPACK_VALUES_PARAM_ID),
            LinkSource::Literal(Value::ArrayValue(vec![Value::Structure(Structure {
                id: Uuid::from_u128(0x50),
                fields: vec![StructureField {
                    id: field,
                    value: Box::new(Value::U64(42)),
                }],
            })])),
        );
        // `Fallback(Equal(.x.field, 42), Sequence(call m, Unpack(→ .x)))`
        // calls `m` once: then `.x` holds the structure.
        let mut graph = Graph::empty();
        graph.root = Some(root);
        graph.nodes.insert(
            root,
            control(root, nodes::FALLBACK_FUNCTION_ID, vec![equal, first]),
        );
        graph.nodes.insert(
            first,
            control(first, nodes::SEQ_FUNCTION_ID, vec![call, unpack]),
        );
        graph
            .nodes
            .insert(equal, leaf_node(equal, nodes::EQUAL_FUNCTION_ID));
        graph
            .nodes
            .insert(unpack, leaf_node(unpack, nodes::UNPACK_FUNCTION_ID));
        add_call(&mut graph, call, m);
        graph.links.extend([
            Link::new(
                Port::new(equal, nodes::EQUAL_A_PARAM_ID),
                LinkSource::Select {
                    source: Box::new(LinkSource::Variable(x)),
                    path: Key::new(format!(".{field}")),
                },
            ),
            Link::new(
                Port::new(equal, nodes::EQUAL_B_PARAM_ID),
                LinkSource::Literal(Value::U64(42)),
            ),
            values.clone(),
            Link::new(
                Port::new(unpack, nodes::UNPACK_OUT_PARAM_IDS[0]),
                LinkSource::Variable(x),
            ),
        ]);
        graph.variables.insert(x, ".x".to_string());
        device.interp.load(graph).unwrap();
        device.tick();
        device.tick();
        assert_eq!(device.calls(m), 1);

        // Relinking `Unpack` restarts it; `Equal` reads `.x` through a
        // selection and keeps it.
        device
            .interp
            .apply(GraphDiff {
                add_links: vec![values],
                ..GraphDiff::default()
            })
            .unwrap();
        device.tick();
        assert_eq!(device.calls(m), 1, ".x kept its value");
    }
}

/// The interpreter module's own functions are not reachable from a tree: a
/// Groot tag does not resolve to them, and a spawned graph calling one is
/// refused.
#[test]
fn a_tree_does_not_call_the_interpreter_module_s_functions() {
    use crate::behavior::{graph_type, BehaviorTreeInterpreter};
    use crate::schema_groot::BehaviorTree as GrootTree;
    use arora_behavior::graph::{Graph, Node as GraphNode};
    use arora_behavior::{interpreter_module, BehaviorInterpreter, RunPolicy};

    let halt = ModuleFunction {
        module_id: interpreter_module::ID,
        function_id: interpreter_module::HALT,
        function_name: "halt".to_string(),
        function: Function {
            parameters: HashMap::new(),
            parameter_ordering: Vec::new(),
            return_ty: FrozenTy::from(PrimitiveKind::Unit),
        },
    };
    let index = HashMap::from([(interpreter_module::HALT, halt)]);
    let xml = r#"<root main_tree_to_execute="MainTree">
  <BehaviorTree ID="MainTree">
    <Halt/>
  </BehaviorTree>
</root>"#;
    let groot = GrootTree::try_from_groot_xml(xml).expect("parse");
    assert!(groot.into_graph(&index).is_err(), "Halt does not resolve");

    let node = Uuid::from_u128(0x1);
    let mut graph = Graph::empty();
    graph.root = Some(node);
    graph.nodes.insert(
        node,
        GraphNode {
            id: node,
            function: interpreter_module::HALT,
            ..GraphNode::default()
        },
    );
    let mut interp = BehaviorTreeInterpreter::new();
    assert!(interp
        .spawn_graph(&graph_type(), graph, RunPolicy::Concurrent)
        .is_err());
}

/// The core string nodes run natively in a Groot tree over a data store, with
/// no module loaded: their ports bind to store keys that need not exist yet.
mod core_string_nodes {
    use super::{LeafStatuses, LeafTicks, TestBridge};
    use crate::behavior::BehaviorTreeInterpreter;
    use crate::nodes;
    use crate::schema::Expression;
    use crate::variable::VariableCell;
    use crate::BehaviorTreeRuntime;
    use arora_behavior::Status;
    use arora_behavior::{BehaviorContext, BehaviorInterpreter};
    use arora_simple_data_store::SimpleDataStore;
    use arora_types::data::{DataStore, Key, StateChange};
    use arora_types::value::Value;
    use std::collections::HashMap;

    fn groot(body: &str) -> String {
        format!(
            r#"<root main_tree_to_execute="MainTree"><BehaviorTree ID="MainTree">{body}</BehaviorTree></root>"#
        )
    }

    /// An interpreter with no module (an empty function index) running `body`.
    fn interpreter(body: &str) -> BehaviorTreeInterpreter {
        let mut interpreter = BehaviorTreeInterpreter::new();
        interpreter
            .load_groot(&groot(body), &HashMap::new())
            .expect("the core nodes need no module");
        interpreter
    }

    /// A loaded tree and the bridge its node ticks are registered on.
    struct Device {
        tree: BehaviorTreeInterpreter,
        bridge: TestBridge,
    }

    fn device(body: &str) -> Device {
        Device {
            tree: interpreter(body),
            bridge: TestBridge::new(LeafStatuses::default(), LeafTicks::default()),
        }
    }

    fn tick(device: &mut Device, store: &SimpleDataStore) {
        let mut ctx = BehaviorContext {
            functions: &arora_behavior::FunctionIndex::new(),
            store,
            call_bridge: &mut device.bridge,
        };
        device.tree.tick(&mut ctx).expect("the tree ticks");
    }

    fn read(store: &SimpleDataStore, key: &str) -> Option<Value> {
        store.read(&[Key::from(key)]).remove(0)
    }

    fn string(value: &str) -> Option<Value> {
        Some(Value::String(value.to_string()))
    }

    fn unset(key: &str) -> StateChange {
        StateChange {
            set: HashMap::new(),
            unset: std::collections::HashSet::from([Key::from(key)]),
        }
    }

    /// `IsStringSet → SetString → UnsetString` on a key written from outside
    /// the tree runs its branch once per write: the key is absent at first
    /// (the condition fails, nothing errors), each write runs the branch once,
    /// and the branch consuming the key keeps it from running again.
    #[test]
    fn a_branch_runs_once_per_write_of_a_key() {
        let store = SimpleDataStore::new();
        let mut tree = device(
            r#"<Sequence>
                 <IsStringSet value="{message}"/>
                 <SetString variable="{heard}" value="{message}"/>
                 <UnsetString variable="{message}"/>
               </Sequence>"#,
        );

        tick(&mut tree, &store);
        assert_eq!(read(&store, "heard"), None, "no message yet: no branch");

        store
            .write(StateChange::set("message", Value::String("hi".into())))
            .unwrap();
        tick(&mut tree, &store);
        assert_eq!(read(&store, "heard"), string("hi"), "the branch ran");
        assert_eq!(read(&store, "message"), None, "and consumed the message");

        // Clear the trace: a tick without a new write does not run the branch.
        store.write(unset("heard")).unwrap();
        tick(&mut tree, &store);
        tick(&mut tree, &store);
        assert_eq!(read(&store, "heard"), None, "no write, no run");

        store
            .write(StateChange::set("message", Value::String("again".into())))
            .unwrap();
        tick(&mut tree, &store);
        assert_eq!(read(&store, "heard"), string("again"));
        assert_eq!(read(&store, "message"), None);

        // An empty string is not set.
        store.write(unset("heard")).unwrap();
        store
            .write(StateChange::set("message", Value::String(String::new())))
            .unwrap();
        tick(&mut tree, &store);
        assert_eq!(read(&store, "heard"), None, "an empty string is not set");
    }

    /// `SetString` creates a key that does not exist yet, from a literal.
    #[test]
    fn set_string_creates_a_missing_key() {
        let store = SimpleDataStore::new();
        let mut tree = device(r#"<SetString variable="{greeting}" value="hello"/>"#);
        assert_eq!(read(&store, "greeting"), None);
        tick(&mut tree, &store);
        assert_eq!(read(&store, "greeting"), string("hello"));
    }

    /// `WaitStringSet` holds its sequence `Running` while its key is absent,
    /// then lets it through once the key is written.
    #[test]
    fn wait_string_set_waits_for_its_key() {
        let store = SimpleDataStore::new();
        let mut tree = device(
            r#"<Sequence>
                 <WaitStringSet value="{reply}"/>
                 <SetString variable="{done}" value="yes"/>
               </Sequence>"#,
        );
        tick(&mut tree, &store);
        tick(&mut tree, &store);
        assert_eq!(read(&store, "done"), None, "waiting for the reply");
        store
            .write(StateChange::set("reply", Value::String("ok".into())))
            .unwrap();
        tick(&mut tree, &store);
        assert_eq!(read(&store, "done"), string("yes"));
    }

    /// `WaitStringSet`'s own status: `Running` on an absent value, `Success`
    /// once set.
    #[test]
    fn wait_string_set_is_running_then_success() {
        let reply = VariableCell::local();
        let tree = crate::tests::build(nodes::wait_str_set(Expression::Variable(reply.clone())));
        let mut bridge = TestBridge::empty();
        let mut runtime = BehaviorTreeRuntime::setup(&tree, &HashMap::new(), &mut bridge, false)
            .expect("runtime sets up");
        assert_eq!(runtime.tick().unwrap(), Status::Running);
        assert_eq!(runtime.tick().unwrap(), Status::Running);
        reply.set(Value::String("ok".into()));
        assert_eq!(runtime.tick().unwrap(), Status::Success);
    }

    /// `RegexMatch` writes the matched text to its output key, creating it,
    /// and fails — leaving the output alone — when nothing matches or the
    /// input is absent.
    #[test]
    fn regex_match_writes_the_match() {
        let store = SimpleDataStore::new();
        let mut tree = device(
            r#"<RegexMatch value="{input}" matcher="(Ross|Brad|Victor)" first_match="{name}"/>"#,
        );
        tick(&mut tree, &store);
        assert_eq!(read(&store, "name"), None, "no input: no match");
        store
            .write(StateChange::set(
                "input",
                Value::String("I am Victor".into()),
            ))
            .unwrap();
        tick(&mut tree, &store);
        assert_eq!(read(&store, "name"), string("Victor"));
        store
            .write(StateChange::set("input", Value::String("nobody".into())))
            .unwrap();
        tick(&mut tree, &store);
        assert_eq!(read(&store, "name"), string("Victor"), "a miss leaves it");
    }

    /// A port bound to a value that is not a string fails the tick, as the
    /// module's argument decoding did, rather than reading as unset.
    #[test]
    fn a_non_string_value_fails_the_tick() {
        let value = VariableCell::local_with(Value::F32(1.0));
        let tree = crate::tests::build(nodes::is_str_set(Expression::Variable(value)));
        let mut bridge = TestBridge::empty();
        let mut runtime = BehaviorTreeRuntime::setup(&tree, &HashMap::new(), &mut bridge, false)
            .expect("runtime sets up");
        assert!(runtime.tick().is_err());
    }
}

/// Module functions with optional parameters, bound to store keys that may be
/// absent: an absent key passes `None`, an optional out-parameter writes
/// `Some` as the key's value and `None` as unsetting it.
mod optional_parameters {
    use crate::behavior::BehaviorTreeInterpreter;
    use crate::ModuleFunction;
    use arora_behavior::{BehaviorContext, BehaviorInterpreter};
    use arora_simple_data_store::SimpleDataStore;
    use arora_types::call::{Call, CallBridge, CallError, CallResult, Callable, CallableId};
    use arora_types::data::{DataStore, Key, StateChange};
    use arora_types::record::module::frozen::{Function, Parameter};
    use arora_types::record::ty::{FrozenOption, FrozenTy, PrimitiveKind};
    use arora_types::value::{StructureField, Value};
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::rc::Rc;
    use uuid::Uuid;

    const ECHO: Uuid = Uuid::from_u128(0xEC40);
    const INPUT: Uuid = Uuid::from_u128(0xEC41);
    const OUTPUT: Uuid = Uuid::from_u128(0xEC42);

    fn unset(key: &str) -> StateChange {
        StateChange {
            set: HashMap::new(),
            unset: std::collections::HashSet::from([Key::from(key)]),
        }
    }

    fn optional_string() -> FrozenTy {
        FrozenTy::FrozenOption(FrozenOption {
            element: Box::new(FrozenTy::from(PrimitiveKind::String)),
        })
    }

    /// `echo(input: Option<String>, output: &mut Option<String>)`.
    fn echo_index() -> HashMap<Uuid, ModuleFunction> {
        HashMap::from([(
            ECHO,
            ModuleFunction {
                module_id: Uuid::from_u128(0xEC00),
                function_id: ECHO,
                function_name: "echo".to_string(),
                function: Function {
                    parameters: HashMap::from([
                        (
                            INPUT,
                            Parameter {
                                name: "input".to_string(),
                                ty: optional_string(),
                                mutable: false,
                            },
                        ),
                        (
                            OUTPUT,
                            Parameter {
                                name: "output".to_string(),
                                ty: optional_string(),
                                mutable: true,
                            },
                        ),
                    ]),
                    parameter_ordering: vec![INPUT, OUTPUT],
                    return_ty: FrozenTy::from(PrimitiveKind::U8),
                },
            },
        )])
    }

    /// Serves `echo` the way a module decoding `Option<String>` would: it
    /// refuses `()` for its optional input, and returns its output framed as
    /// an optional. Records the inputs it received.
    #[derive(Default)]
    struct EchoBridge {
        registered: HashMap<u64, Rc<dyn Callable>>,
        next_id: u64,
        inputs: Rc<RefCell<Vec<Value>>>,
    }

    impl CallBridge for EchoBridge {
        fn arora_call(&mut self, call: Call) -> Result<CallResult, CallError> {
            let input = call
                .args
                .iter()
                .find(|arg| arg.id == INPUT)
                .map(|arg| (*arg.value).clone())
                .ok_or(CallError::Generic {
                    message: "no input".into(),
                })?;
            self.inputs.borrow_mut().push(input.clone());
            let output = match input {
                Value::Option(None) => Value::Option(None),
                Value::Option(Some(s)) if matches!(*s, Value::String(_)) => Value::Option(Some(s)),
                other => {
                    return Err(CallError::Generic {
                        message: format!("input: unexpected value {other}"),
                    })
                }
            };
            Ok(CallResult {
                ret: Value::Unit,
                mutated: vec![StructureField {
                    id: OUTPUT,
                    value: Box::new(output),
                }],
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

    /// A tree resolves what it calls against the index of the tick that
    /// lowers it: a function that index does not have fails the node's tick
    /// as not found, without reaching the module.
    #[test]
    fn a_function_the_tick_s_index_lacks_is_not_found() {
        let mut tree = BehaviorTreeInterpreter::new();
        tree.load_groot(
            r#"<root main_tree_to_execute="MainTree"><BehaviorTree ID="MainTree">
                 <Echo input="{in}" output="{out}"/>
               </BehaviorTree></root>"#,
            &echo_index(),
        )
        .expect("echo resolves by its PascalCase name");
        let store = SimpleDataStore::new();
        let mut bridge = EchoBridge::default();
        let inputs = bridge.inputs.clone();
        let mut ctx = BehaviorContext {
            functions: &arora_behavior::FunctionIndex::new(),
            store: &store,
            call_bridge: &mut bridge,
        };
        let error = tree.tick(&mut ctx).expect_err("echo is not in the index");
        assert!(error.message.contains("FunctionNotFound"), "{error}");
        assert!(inputs.borrow().is_empty(), "the module is not called");
    }

    #[test]
    fn an_absent_key_binds_none_and_an_optional_output_maps_to_the_key() {
        let store = SimpleDataStore::new();
        let functions = echo_index();
        let mut tree = BehaviorTreeInterpreter::new();
        tree.load_groot(
            r#"<root main_tree_to_execute="MainTree"><BehaviorTree ID="MainTree">
                 <Echo input="{in}" output="{out}"/>
               </BehaviorTree></root>"#,
            &functions,
        )
        .expect("echo resolves by its PascalCase name");
        let mut bridge = EchoBridge::default();
        let inputs = bridge.inputs.clone();
        let read = |key: &str| store.read(&[Key::from(key)]).remove(0);
        let mut tick = |bridge: &mut EchoBridge| {
            let mut ctx = BehaviorContext {
                functions: &functions,
                store: &store,
                call_bridge: bridge,
            };
            tree.tick(&mut ctx).expect("the tree ticks");
        };

        // `in` is not set: the module receives `None`, not `()`, and writes
        // `None`, which leaves `out` absent.
        tick(&mut bridge);
        assert_eq!(inputs.borrow().last(), Some(&Value::Option(None)));
        assert_eq!(read("out"), None);

        // `in` is set: the module receives `Some("x")`, and its `Some`
        // creates `out`, holding the bare string.
        store
            .write(StateChange::set("in", Value::String("x".into())))
            .unwrap();
        tick(&mut bridge);
        assert_eq!(
            inputs.borrow().last(),
            Some(&Value::Option(Some(Box::new(Value::String("x".into())))))
        );
        assert_eq!(read("out"), Some(Value::String("x".into())));

        // `in` is unset again: `None` unsets `out`.
        store.write(unset("in")).unwrap();
        tick(&mut bridge);
        assert_eq!(inputs.borrow().last(), Some(&Value::Option(None)));
        assert_eq!(read("out"), None);
    }
}

/// The data nodes: `Equal` as a condition, `Unpack` spreading an array over
/// variables.
mod data_nodes {
    use crate::behavior::BehaviorTreeInterpreter;
    use crate::nodes::{self, UNPACK_OUT_PARAM_IDS as OUT};
    use arora_behavior::graph::{Graph, Io, Link, LinkSource, Node as GraphNode, Port};
    use arora_behavior::{BehaviorContext, BehaviorInterpreter};
    use arora_simple_data_store::SimpleDataStore;
    use arora_types::data::{DataStore, Key, StateChange};
    use arora_types::gen_uuid_from_str;
    use arora_types::value::{Structure, StructureField, Value};
    use std::collections::HashMap;
    use std::rc::Rc;
    use uuid::Uuid;

    const SEQ: Uuid = Uuid::from_u128(0x100);
    const EQUAL: Uuid = Uuid::from_u128(0x101);
    const UNPACK: Uuid = Uuid::from_u128(0x102);
    const MARK: Uuid = Uuid::from_u128(0x103);

    fn node(id: Uuid, function: Uuid, inputs: &[Uuid]) -> GraphNode {
        GraphNode {
            id,
            function,
            inputs: inputs.iter().copied().map(Io::new).collect(),
            ..GraphNode::default()
        }
    }

    /// The variable named `name`.
    fn variable(name: &str) -> Uuid {
        gen_uuid_from_str(name)
    }

    /// `Sequence(Equal(a, b), Unpack(values, out_i → {name}…), Unpack([true],
    /// out_0 → {done}))`: the last node marks that the sequence got past the
    /// first two.
    fn guarded_unpack(
        a: LinkSource,
        b: LinkSource,
        outputs: &[(usize, &str)],
        values: LinkSource,
    ) -> Graph {
        let mut graph = Graph::empty();
        graph.root = Some(SEQ);
        graph.nodes.insert(
            SEQ,
            GraphNode {
                children: Some(vec![EQUAL, UNPACK, MARK]),
                ..node(SEQ, nodes::SEQ_FUNCTION_ID, &[])
            },
        );
        graph.nodes.insert(
            EQUAL,
            node(
                EQUAL,
                nodes::EQUAL_FUNCTION_ID,
                &[nodes::EQUAL_A_PARAM_ID, nodes::EQUAL_B_PARAM_ID],
            ),
        );
        for id in [UNPACK, MARK] {
            graph.nodes.insert(
                id,
                node(
                    id,
                    nodes::UNPACK_FUNCTION_ID,
                    &[nodes::UNPACK_VALUES_PARAM_ID],
                ),
            );
        }
        let link = |node, port, source| Link::new(Port::new(node, port), source);
        graph.links = vec![
            link(EQUAL, nodes::EQUAL_A_PARAM_ID, a),
            link(EQUAL, nodes::EQUAL_B_PARAM_ID, b),
            link(UNPACK, nodes::UNPACK_VALUES_PARAM_ID, values),
            link(
                MARK,
                nodes::UNPACK_VALUES_PARAM_ID,
                LinkSource::Literal(Value::ArrayBoolean(vec![true])),
            ),
            link(MARK, OUT[0], LinkSource::Variable(variable("done"))),
        ];
        for (index, name) in outputs {
            graph.links.push(link(
                UNPACK,
                OUT[*index],
                LinkSource::Variable(variable(name)),
            ));
        }
        for name in outputs.iter().map(|(_, name)| *name).chain(["done"]) {
            graph.variables.insert(variable(name), name.to_string());
        }
        graph
    }

    /// Load `graph` as the main behavior over `store` and tick it once.
    fn tick_once(store: &SimpleDataStore, graph: Graph) -> Result<(), String> {
        let mut interp = BehaviorTreeInterpreter::new();
        interp.load(graph).map_err(|e| e.message)?;
        let mut bridge = super::TestBridge::empty();
        let mut ctx = BehaviorContext {
            functions: &arora_behavior::FunctionIndex::new(),
            store,
            call_bridge: &mut bridge,
        };
        interp.tick(&mut ctx).map(|_| ()).map_err(|e| e.message)
    }

    fn read(store: &SimpleDataStore, keys: &[&str]) -> Vec<Option<Value>> {
        let keys: Vec<Key> = keys.iter().map(|key| Key::new(*key)).collect();
        store.read(&keys)
    }

    fn literal(value: Value) -> LinkSource {
        LinkSource::Literal(value)
    }

    #[test]
    fn unpack_writes_each_element_to_its_port_skipping_unbound_ports_and_units() {
        let store = SimpleDataStore::new();
        let values = Value::ArrayValue(vec![
            Value::F64(1.0),
            Value::F64(2.0),
            Value::Unit,
            Value::String("four".to_string()),
            Value::F64(5.0),
        ]);
        tick_once(
            &store,
            guarded_unpack(
                literal(Value::U64(7)),
                literal(Value::U64(7)),
                &[(0, "out/a"), (2, "out/c"), (3, "out/d")],
                literal(values),
            ),
        )
        .unwrap();
        assert_eq!(
            read(&store, &["out/a", "out/c", "out/d", "done"]),
            vec![
                Some(Value::F64(1.0)),
                None,
                Some(Value::String("four".to_string())),
                Some(Value::Boolean(true)),
            ],
            "out_1 is unbound, the Unit element is skipped, the fifth is past the last port"
        );
    }

    #[test]
    fn the_last_port_takes_the_last_element() {
        let store = SimpleDataStore::new();
        let last = nodes::UNPACK_PORTS - 1;
        let values = (0..nodes::UNPACK_PORTS).map(|i| i as f64).collect();
        tick_once(
            &store,
            guarded_unpack(
                literal(Value::U8(1)),
                literal(Value::U8(1)),
                &[(last, "out/last")],
                literal(Value::ArrayF64(values)),
            ),
        )
        .unwrap();
        assert_eq!(
            read(&store, &["out/last"]),
            vec![Some(Value::F64(last as f64))]
        );
    }

    #[test]
    fn equal_fails_the_sequence_when_the_values_differ() {
        let store = SimpleDataStore::new();
        tick_once(
            &store,
            guarded_unpack(
                literal(Value::U64(7)),
                literal(Value::U64(8)),
                &[(0, "out/a")],
                literal(Value::ArrayF64(vec![1.0])),
            ),
        )
        .unwrap();
        assert_eq!(read(&store, &["out/a", "done"]), vec![None, None]);
    }

    #[test]
    fn unpack_fails_and_writes_nothing_on_a_short_array_or_a_non_array() {
        for values in [Value::ArrayF64(vec![1.0]), Value::F64(1.0), Value::Unit] {
            let store = SimpleDataStore::new();
            tick_once(
                &store,
                guarded_unpack(
                    literal(Value::U8(1)),
                    literal(Value::U8(1)),
                    &[(0, "out/a"), (1, "out/b")],
                    literal(values.clone()),
                ),
            )
            .unwrap();
            assert_eq!(
                read(&store, &["out/a", "out/b", "done"]),
                vec![None, None, None],
                "{values} writes nothing and fails"
            );
        }
    }

    /// The stepping pattern: a structure read from the store, its revision
    /// compared by selection, its values unpacked by selection.
    #[test]
    fn selections_feed_equal_and_unpack() {
        let revision = Uuid::from_u128(0x51);
        let values = Uuid::from_u128(0x52);
        let frame = Uuid::from_u128(0x53);
        let store = SimpleDataStore::new();
        store
            .write(StateChange::set(
                "frame",
                Value::Structure(Structure {
                    id: Uuid::from_u128(0x50),
                    fields: vec![
                        StructureField {
                            id: revision,
                            value: Box::new(Value::U64(3)),
                        },
                        StructureField {
                            id: values,
                            value: Box::new(Value::ArrayF64(vec![0.5, 0.25])),
                        },
                    ],
                }),
            ))
            .unwrap();
        let select = |field: Uuid| LinkSource::Select {
            source: Box::new(LinkSource::Variable(frame)),
            path: Key::new(format!(".{field}")),
        };
        let mut graph = guarded_unpack(
            select(revision),
            literal(Value::U64(3)),
            &[(0, "face/x"), (1, "face/y")],
            select(values),
        );
        graph.variables.insert(frame, "frame".to_string());
        tick_once(&store, graph).unwrap();
        assert_eq!(
            read(&store, &["face/x", "face/y", "done"]),
            vec![
                Some(Value::F64(0.5)),
                Some(Value::F64(0.25)),
                Some(Value::Boolean(true))
            ]
        );
    }

    /// Lowering checks the data nodes: both of `Equal`'s arguments and
    /// `Unpack`'s values linked, each of its outputs a port bound to a
    /// variable no other output binds.
    #[test]
    fn data_nodes_are_checked_at_lowering() {
        let graph = |outputs: &[(usize, &str)]| {
            guarded_unpack(
                literal(Value::U8(1)),
                literal(Value::U8(1)),
                outputs,
                literal(Value::ArrayF64(vec![1.0, 2.0])),
            )
        };
        let unlinked = |node: Uuid, port: Uuid| {
            let mut graph = graph(&[(0, "out/a")]);
            graph
                .links
                .retain(|link| link.target != Port::new(node, port));
            graph
        };
        let bound = |port: Uuid, source: LinkSource| {
            let mut graph = graph(&[]);
            graph.links.push(Link::new(Port::new(UNPACK, port), source));
            graph
        };
        // Two variables of one name: one store key.
        let mut one_name = graph(&[(0, "out/a")]);
        let other = Uuid::from_u128(0x77);
        one_name.variables.insert(other, "out/a".to_string());
        one_name.links.push(Link::new(
            Port::new(UNPACK, OUT[1]),
            LinkSource::Variable(other),
        ));
        for (what, graph) in [
            ("Equal without b", unlinked(EQUAL, nodes::EQUAL_B_PARAM_ID)),
            (
                "Unpack without values",
                unlinked(UNPACK, nodes::UNPACK_VALUES_PARAM_ID),
            ),
            (
                "two ports on one variable",
                graph(&[(0, "out/a"), (1, "out/a")]),
            ),
            ("two ports on one name", one_name),
            (
                "an output bound to a literal",
                bound(OUT[0], literal(Value::F64(1.0))),
            ),
            (
                "an output bound to a selection",
                bound(
                    OUT[0],
                    LinkSource::Select {
                        source: Box::new(LinkSource::Variable(variable("done"))),
                        path: Key::new(".0"),
                    },
                ),
            ),
            (
                "an output bound to another node's port",
                bound(
                    OUT[0],
                    LinkSource::Port(Port::new(EQUAL, nodes::EQUAL_A_PARAM_ID)),
                ),
            ),
            (
                "a port Unpack does not have",
                bound(
                    Uuid::from_u128(0x78),
                    LinkSource::Variable(variable("done")),
                ),
            ),
        ] {
            let store = SimpleDataStore::new();
            assert!(tick_once(&store, graph).is_err(), "{what} does not lower");
        }
    }

    /// Without a store, an output binds the tree's own variable, so a node
    /// reading the variable sees the write.
    #[test]
    fn an_output_shares_the_cell_of_its_variable() {
        use crate::graph::build_behavior_tree;
        let mut graph = guarded_unpack(
            literal(Value::U8(1)),
            literal(Value::U8(1)),
            &[(0, "k")],
            literal(Value::ArrayU8(vec![5])),
        );
        // A check after the writes: Equal({k}, 5).
        let check = Uuid::from_u128(0x104);
        graph.nodes.get_mut(&SEQ).unwrap().children = Some(vec![EQUAL, UNPACK, MARK, check]);
        graph.nodes.insert(
            check,
            node(
                check,
                nodes::EQUAL_FUNCTION_ID,
                &[nodes::EQUAL_A_PARAM_ID, nodes::EQUAL_B_PARAM_ID],
            ),
        );
        graph.links.push(Link::new(
            Port::new(check, nodes::EQUAL_A_PARAM_ID),
            LinkSource::Variable(variable("k")),
        ));
        graph.links.push(Link::new(
            Port::new(check, nodes::EQUAL_B_PARAM_ID),
            literal(Value::U8(5)),
        ));
        let tree = build_behavior_tree(&graph, &|_| None).expect("the tree lowers");
        let mut bridge = super::TestBridge::empty();
        let status = crate::run_behavior_tree(&tree, &HashMap::new(), &mut bridge, false).unwrap();
        assert_eq!(status, arora_behavior::Status::Success);
    }

    /// An `Unpack` at the root of a tree built from `TreeNode`s gets its
    /// outputs too.
    #[test]
    fn a_root_unpack_built_from_tree_nodes_runs() {
        use crate::schema::Expression;
        use crate::tree_node::TreeNode;
        let tree = super::build(TreeNode {
            function: nodes::UNPACK_FUNCTION_ID,
            children: None,
            parameters: HashMap::from([
                (
                    nodes::UNPACK_VALUES_PARAM_ID,
                    Expression::Value(Value::ArrayF64(vec![1.0])),
                ),
                (OUT[0], Expression::VariableId(variable("out/a"))),
            ]),
        });
        assert_eq!(super::run(&tree), arora_behavior::Status::Success);
    }

    /// An edit that would leave an `Unpack` unlowerable is refused, and the
    /// graph stays as it was.
    #[test]
    fn an_edit_breaking_an_unpack_is_refused() {
        use arora_behavior::graph::GraphDiff;
        let mut interp = BehaviorTreeInterpreter::new();
        interp
            .load(guarded_unpack(
                literal(Value::U8(1)),
                literal(Value::U8(1)),
                &[(0, "out/a")],
                literal(Value::ArrayF64(vec![1.0])),
            ))
            .unwrap();
        let before = interp.graph().clone();
        for diff in [
            GraphDiff {
                remove_links: vec![Port::new(UNPACK, nodes::UNPACK_VALUES_PARAM_ID)],
                ..GraphDiff::default()
            },
            GraphDiff {
                add_links: vec![Link::new(
                    Port::new(UNPACK, OUT[1]),
                    LinkSource::Variable(variable("out/a")),
                )],
                ..GraphDiff::default()
            },
        ] {
            assert!(interp.apply(diff).is_err());
            assert_eq!(interp.graph(), &before);
        }
    }

    /// A value that does not move makes no store change — the store notifies
    /// changes only — so unpacking the same values twice changes each key
    /// once, and a moved value changes it again.
    #[test]
    fn unpack_writes_a_key_only_when_its_value_moves() {
        use arora_behavior::graph::GraphDiff;
        let store = SimpleDataStore::new();
        let mut interp = BehaviorTreeInterpreter::new();
        interp
            .load(guarded_unpack(
                literal(Value::U8(1)),
                literal(Value::U8(1)),
                &[(0, "out/a"), (1, "out/b")],
                literal(Value::ArrayF64(vec![1.0, 2.0])),
            ))
            .unwrap();
        let mut bridge = super::TestBridge::empty();
        let changes = store.subscribe();
        // The changes per key since the last drain.
        let drain = || {
            let mut counts: HashMap<String, usize> = HashMap::new();
            while let Some(change) = changes.try_recv() {
                for key in change.set.keys() {
                    *counts.entry(key.path.clone()).or_default() += 1;
                }
            }
            counts
        };
        let mut tick = |interp: &mut BehaviorTreeInterpreter| {
            let mut ctx = BehaviorContext {
                functions: &arora_behavior::FunctionIndex::new(),
                store: &store,
                call_bridge: &mut bridge,
            };
            interp.tick(&mut ctx).unwrap();
        };
        tick(&mut interp);
        tick(&mut interp);
        let counts = drain();
        assert_eq!(counts.get("out/a"), Some(&1), "changed once over two ticks");
        assert_eq!(counts.get("out/b"), Some(&1), "changed once over two ticks");

        interp
            .apply(GraphDiff {
                add_links: vec![Link::new(
                    Port::new(UNPACK, nodes::UNPACK_VALUES_PARAM_ID),
                    literal(Value::ArrayF64(vec![1.0, 3.0])),
                )],
                ..GraphDiff::default()
            })
            .unwrap();
        tick(&mut interp);
        let counts = drain();
        assert_eq!(counts.get("out/a"), None, "the held value makes no change");
        assert_eq!(counts.get("out/b"), Some(&1), "the moved value changes");
        assert_eq!(read(&store, &["out/b"]), vec![Some(Value::F64(3.0))]);
    }

    /// A module leaf's argument linked through a selection receives the
    /// selected part of the source, not the whole value.
    #[test]
    fn a_module_leaf_receives_the_selected_part_of_its_argument() {
        use crate::ModuleFunction;
        use arora_types::call::{Call, CallBridge, CallError, CallResult, Callable, CallableId};
        use arora_types::record::module::frozen::Function;
        use arora_types::record::ty::{FrozenTy, PrimitiveKind};
        use std::cell::RefCell;

        /// Records the arguments of each module call, and answers Success.
        #[derive(Default)]
        struct Recording {
            calls: Rc<RefCell<Vec<Call>>>,
            registered: HashMap<u64, Rc<dyn Callable>>,
        }
        impl CallBridge for Recording {
            fn arora_call(&mut self, call: Call) -> Result<CallResult, CallError> {
                self.calls.borrow_mut().push(call);
                Ok(CallResult {
                    ret: arora_behavior::Status::Success.into(),
                    mutated: Vec::new(),
                })
            }
            fn arora_register_callable(&mut self, callable: Rc<dyn Callable>) -> CallableId {
                let id = self.registered.len() as u64;
                self.registered.insert(id, callable);
                CallableId { id }
            }
            fn arora_unregister_callable(&mut self, callable_id: &CallableId) {
                self.registered.remove(&callable_id.id);
            }
            fn arora_call_indirect(&mut self, id: &CallableId) -> Result<Value, CallError> {
                let callable = self.registered[&id.id].clone();
                callable.call(self)
            }
        }

        let function = Uuid::from_u128(0xF00);
        let param = Uuid::from_u128(0xF01);
        let field = Uuid::from_u128(0xF02);
        let frame = Uuid::from_u128(0xF03);
        let leaf = Uuid::from_u128(0xF04);
        let producer = Uuid::from_u128(0xF06);
        let produced = Uuid::from_u128(0xF07);
        let index = |mutable: bool| {
            use arora_types::record::module::frozen::Parameter;
            HashMap::from([(
                function,
                ModuleFunction {
                    module_id: Uuid::from_u128(0xF05),
                    function_id: function,
                    function_name: "consume".to_string(),
                    function: Function {
                        parameters: HashMap::from([(
                            param,
                            Parameter {
                                name: "frame".to_string(),
                                ty: FrozenTy::from(PrimitiveKind::U64),
                                mutable,
                            },
                        )]),
                        parameter_ordering: vec![param],
                        return_ty: FrozenTy::from(PrimitiveKind::U8),
                    },
                },
            )])
        };
        let structure = Value::Structure(Structure {
            id: Uuid::from_u128(0x50),
            fields: vec![StructureField {
                id: field,
                value: Box::new(Value::U64(42)),
            }],
        });
        let select = |source: LinkSource| LinkSource::Select {
            source: Box::new(source),
            path: Key::new(format!(".{field}")),
        };
        // `Sequence(producer, leaf)`: the producer holds the structure in an
        // argument, which a port link can read.
        let graph = |source: LinkSource, target: Uuid| {
            let mut graph = Graph::empty();
            graph.root = Some(SEQ);
            graph.nodes.insert(
                SEQ,
                GraphNode {
                    children: Some(vec![producer, leaf]),
                    ..node(SEQ, nodes::SEQ_FUNCTION_ID, &[])
                },
            );
            graph.nodes.insert(
                producer,
                node(producer, nodes::SUCCEED_FUNCTION_ID, &[produced]),
            );
            graph.nodes.insert(leaf, node(leaf, function, &[param]));
            graph.links.push(Link::new(
                Port::new(producer, produced),
                literal(structure.clone()),
            ));
            graph.links.push(Link::new(Port::new(leaf, target), source));
            graph.variables.insert(frame, "frame".to_string());
            graph
        };
        // What the leaf receives, with the store holding `stored` under
        // `frame`.
        let received = |graph: Graph, stored: Option<Value>| {
            let store = SimpleDataStore::new();
            if let Some(stored) = stored {
                store.write(StateChange::set("frame", stored)).unwrap();
            }
            let functions = index(false);
            let mut interp = BehaviorTreeInterpreter::new();
            interp.load(graph).unwrap();
            let mut bridge = Recording::default();
            let calls = bridge.calls.clone();
            let mut ctx = BehaviorContext {
                functions: &functions,
                store: &store,
                call_bridge: &mut bridge,
            };
            interp.tick(&mut ctx).unwrap();
            let calls = calls.borrow();
            assert_eq!(calls.len(), 1);
            *calls[0].args[0].value.clone()
        };

        let from_variable = graph(select(LinkSource::Variable(frame)), param);
        assert_eq!(
            received(from_variable.clone(), Some(structure.clone())),
            Value::U64(42)
        );
        assert_eq!(
            received(from_variable.clone(), None),
            Value::Unit,
            "a selection over an absent value reads as absent"
        );
        let from_port = graph(
            select(LinkSource::Port(Port::new(producer, produced))),
            param,
        );
        assert_eq!(received(from_port, None), Value::U64(42));

        // A selection is read-only. On the return binding it is refused at
        // load; on a parameter the device's index declares mutable, the next
        // tick fails, and keeps failing while the graph holds it.
        let on_return = graph(
            select(LinkSource::Variable(frame)),
            crate::schema::_RET_PARAM_ID,
        );
        let mut interp = BehaviorTreeInterpreter::new();
        assert!(interp.load(on_return).is_err(), "the return binding");

        let mut interp = BehaviorTreeInterpreter::new();
        interp
            .load(from_variable)
            .expect("the index is not known at load");
        let store = SimpleDataStore::new();
        let mut bridge = Recording::default();
        let calls = bridge.calls.clone();
        let mut ctx = BehaviorContext {
            functions: &index(true),
            store: &store,
            call_bridge: &mut bridge,
        };
        for _ in 0..2 {
            let error = interp.tick(&mut ctx).expect_err("a mutable parameter");
            assert!(error.message.contains("would be written"), "{error}");
        }
        assert!(calls.borrow().is_empty(), "nothing is called");
    }
}

/// Tree-local variables: a variable whose name starts with `.` is a cell of
/// its lowered tree, never a store key.
mod tree_locals {
    use crate::behavior::BehaviorTreeInterpreter;
    use crate::nodes::{self, UNPACK_OUT_PARAM_IDS as OUT};
    use arora_behavior::graph::{Graph, Io, Link, LinkSource, Node as GraphNode, Port};
    use arora_behavior::{BehaviorContext, BehaviorInterpreter};
    use arora_simple_data_store::SimpleDataStore;
    use arora_types::data::{DataStore, Key};
    use arora_types::value::Value;
    use uuid::Uuid;

    const SEQ: Uuid = Uuid::from_u128(0x200);
    const X: Uuid = Uuid::from_u128(0x2F0);

    /// `Unpack` reading `values`, its outputs bound to `outputs`.
    fn unpack(id: Uuid, values: LinkSource, outputs: &[Uuid]) -> (GraphNode, Vec<Link>) {
        let node = GraphNode {
            id,
            function: nodes::UNPACK_FUNCTION_ID,
            inputs: vec![Io::new(nodes::UNPACK_VALUES_PARAM_ID)],
            ..GraphNode::default()
        };
        let links =
            std::iter::once(Link::new(
                Port::new(id, nodes::UNPACK_VALUES_PARAM_ID),
                values,
            ))
            .chain(outputs.iter().enumerate().map(|(i, variable)| {
                Link::new(Port::new(id, OUT[i]), LinkSource::Variable(*variable))
            }))
            .collect();
        (node, links)
    }

    /// `Sequence(Unpack(values) → out_0: {x}, Unpack({x}) → out_i: {keys[i]})`
    /// with `{x}` named `x_name`, or only the second `Unpack` when `values` is
    /// `None`.
    fn graph(x_name: &str, values: Option<Value>, keys: &[&str]) -> Graph {
        let mut graph = Graph::empty();
        graph.root = Some(SEQ);
        let key_variables: Vec<Uuid> = (0..keys.len())
            .map(|i| Uuid::from_u128(0x2A0 + i as u128))
            .collect();
        let mut parts = Vec::new();
        if let Some(values) = values {
            parts.push(unpack(
                Uuid::from_u128(0x201),
                LinkSource::Literal(values),
                &[X],
            ));
        }
        parts.push(unpack(
            Uuid::from_u128(0x202),
            LinkSource::Variable(X),
            &key_variables,
        ));
        graph.nodes.insert(
            SEQ,
            GraphNode {
                id: SEQ,
                function: nodes::SEQ_FUNCTION_ID,
                children: Some(parts.iter().map(|(node, _)| node.id).collect()),
                ..GraphNode::default()
            },
        );
        for (node, links) in parts {
            graph.nodes.insert(node.id, node);
            graph.links.extend(links);
        }
        graph.variables.insert(X, x_name.to_string());
        for (variable, key) in key_variables.iter().zip(keys) {
            graph.variables.insert(*variable, key.to_string());
        }
        graph
    }

    fn interpreter(graph: Graph) -> BehaviorTreeInterpreter {
        let mut interp = BehaviorTreeInterpreter::new();
        interp.load(graph).unwrap();
        interp
    }

    fn tick(store: &SimpleDataStore, interp: &mut BehaviorTreeInterpreter) {
        let mut bridge = super::TestBridge::empty();
        let mut ctx = BehaviorContext {
            functions: &arora_behavior::FunctionIndex::new(),
            store,
            call_bridge: &mut bridge,
        };
        interp.tick(&mut ctx).unwrap();
    }

    fn read(store: &SimpleDataStore, keys: &[&str]) -> Vec<Option<Value>> {
        let keys: Vec<Key> = keys.iter().map(|key| Key::new(*key)).collect();
        store.read(&keys)
    }

    fn pair() -> Value {
        Value::ArrayValue(vec![Value::ArrayF64(vec![1.0, 2.0])])
    }

    /// The intermediate array passes through `.x`, which no store change
    /// carries.
    #[test]
    fn a_dot_variable_never_creates_a_store_key() {
        let store = SimpleDataStore::new();
        let changes = store.subscribe();
        let mut interp = interpreter(graph(".x", Some(pair()), &["a", "b"]));
        tick(&store, &mut interp);
        assert_eq!(
            read(&store, &["a", "b", ".x"]),
            vec![Some(Value::F64(1.0)), Some(Value::F64(2.0)), None]
        );
        let mut changed = Vec::new();
        while let Some(change) = changes.try_recv() {
            changed.extend(change.set.keys().map(|key| key.path.clone()));
        }
        changed.sort();
        assert_eq!(changed, vec!["a".to_string(), "b".to_string()]);
    }

    /// Two trees over one store each have their own `.x`, even under one
    /// variable id: the tree that never writes its `.x` reads nothing. A
    /// named `x` is one store key, which the second tree reads.
    #[test]
    fn two_trees_dot_x_are_independent() {
        for (name, expected) in [(".x", None), ("x", Some(Value::F64(1.0)))] {
            let store = SimpleDataStore::new();
            let mut writer = interpreter(graph(name, Some(pair()), &["a"]));
            let mut reader = interpreter(graph(name, None, &["b"]));
            tick(&store, &mut writer);
            tick(&store, &mut reader);
            assert_eq!(read(&store, &["a"]), vec![Some(Value::F64(1.0))]);
            assert_eq!(read(&store, &["b"]), vec![expected], "{name}");
        }
    }
}
