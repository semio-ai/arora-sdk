//! Compute nodes: the calls of a node graph held beside the tree, evaluated on
//! demand, once per tick, and only when their inputs changed.
//!
//! The functions are the fake engine's four modules
//! ([`call_expression_tests`](crate::call_expression_tests)). The inputs are
//! store keys, as a node graph reads them.
use std::collections::HashMap;

use arora_simple_data_store::SimpleDataStore;
use arora_types::data::{DataStore, Key, StateChange};
use arora_types::value::Value;
use uuid::Uuid;

use crate::call_expression_tests::{
    centimetres, control, f, index, node, Calls, Engine, ADD, ADD_A, ADD_B, BELOW, BELOW_LIMIT,
    BELOW_X, CLAMP, CLAMP_LO, CLAMP_X, DISTANCE, MUL, MUL_A, MUL_B, SMOOTH, SMOOTH_ALPHA,
    SMOOTH_STATE, SMOOTH_X,
};
use crate::display::Names;
use crate::nodes::{FALLBACK_FUNCTION_ID, PARALLEL_FUNCTION_ID, SEQ_FUNCTION_ID};
use crate::schema::{Expression, Node, NodeParameterId, _RET_PARAM_ID};
use crate::{load_behavior_tree_with_compute, BehaviorTree, BehaviorTreeRuntime, ComputeNode};
use arora_behavior::Status;

/// The variable id of the store key or tree-local name `name`.
pub(crate) fn var(name: &str) -> Uuid {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    name.hash(&mut hasher);
    Uuid::from_u128(u128::from(hasher.finish()) << 64 | 0x7A12)
}

/// The variable `name`: a store key, or tree-local when it starts with `.`.
pub(crate) fn key(name: &str) -> Expression {
    Expression::VariableId(var(name))
}

pub(crate) fn id(node: u128) -> Uuid {
    Uuid::from_u128(node)
}

/// `#node`: a compute node's return value.
pub(crate) fn read(node: u128) -> Expression {
    port(node, _RET_PARAM_ID)
}

/// `#node.parameter`: a mutable parameter's value after the node's last call.
pub(crate) fn port(node: u128, parameter: Uuid) -> Expression {
    Expression::Computed(NodeParameterId {
        node: id(node),
        parameter,
    })
}

pub(crate) fn compute(
    node: u128,
    function: Uuid,
    inputs: Vec<(Uuid, Expression)>,
    output: Option<&str>,
) -> ComputeNode {
    ComputeNode {
        id: id(node),
        function,
        inputs,
        output: output.map(var),
        volatile: false,
    }
}

/// `below(x, limit: limit)`, a leaf of the tree.
pub(crate) fn below(node_id: u128, x: Expression, limit: f64) -> Node {
    node(node_id, BELOW, vec![(BELOW_X, x), (BELOW_LIMIT, f(limit))])
}

/// The names of every variable the tests use, for the resolver and display.
pub(crate) const NAMES: &[&str] = &[
    "sensor/distance",
    "left",
    "right",
    "in",
    "x",
    "y",
    "target",
    "out",
    "face/level",
    ".x",
    ".v",
];

pub(crate) fn names() -> HashMap<Uuid, String> {
    NAMES
        .iter()
        .map(|name| (var(name), name.to_string()))
        .collect()
}

/// Build a tree whose variables are the store's keys, but for the `.` ones.
pub(crate) fn build(
    store: &SimpleDataStore,
    nodes: Vec<Node>,
    compute: Vec<ComputeNode>,
) -> BehaviorTree {
    let resolver = |name: &str| Some(store.slot(&Key::from(name)));
    load_behavior_tree_with_compute(nodes, compute, &resolver, &names()).expect("the tree builds")
}

pub(crate) fn set(store: &SimpleDataStore, name: &str, value: f64) {
    store
        .write(StateChange::set(name, Value::F64(value)))
        .expect("the store writes");
}

pub(crate) fn get(store: &SimpleDataStore, name: &str) -> Option<Value> {
    store.read(&[Key::from(name)]).remove(0)
}

/// How many calls each function received since the last time this was asked.
pub(crate) fn take(calls: &Calls) -> HashMap<&'static str, usize> {
    let mut counts = HashMap::new();
    for (name, _) in calls.borrow_mut().drain(..) {
        *counts.entry(name).or_insert(0) += 1;
    }
    counts
}

/// `#raw = mul(a: {sensor/distance}, b: 100)`, `#cm = clamp(x: #raw, lo: 0)`:
/// the distance in centimetres, never negative.
pub(crate) fn centimetres_nodes() -> Vec<ComputeNode> {
    vec![
        compute(
            10,
            MUL,
            vec![(MUL_A, key("sensor/distance")), (MUL_B, f(100.0))],
            None,
        ),
        compute(
            11,
            CLAMP,
            vec![(CLAMP_X, read(10)), (CLAMP_LO, f(0.0))],
            None,
        ),
    ]
}

/// Three conditions under a parallel, each reading the distance in
/// centimetres against its own limit.
pub(crate) fn three_readers(x: impl Fn() -> Expression) -> Vec<Node> {
    vec![
        control(1, PARALLEL_FUNCTION_ID, &[2, 3, 4]),
        below(2, x(), 10.0),
        below(3, x(), 30.0),
        below(4, x(), 50.0),
    ]
}

/// A node read by three leaves calls its function once per tick. The same
/// cascade written as call expressions in each leaf calls it three times.
#[test]
fn a_node_read_by_several_leaves_calls_its_function_once_per_tick() {
    let store = SimpleDataStore::new();
    set(&store, "sensor/distance", 0.2);

    let nodes = build(&store, three_readers(|| read(11)), centimetres_nodes());
    let mut engine = Engine::default();
    let calls = engine.calls.clone();
    let mut runtime = BehaviorTreeRuntime::setup(&nodes, &index(), &mut engine, false).unwrap();
    runtime.tick().unwrap();
    let counts = take(&calls);
    assert_eq!((counts["mul"], counts["clamp"], counts["below"]), (1, 1, 3));

    // The same cascade inlined: `distance()` is the sensor read as a function.
    let inlined = build(&store, three_readers(centimetres), Vec::new());
    let mut engine = Engine::default();
    engine.set(DISTANCE, 0.2);
    let calls = engine.calls.clone();
    let mut runtime = BehaviorTreeRuntime::setup(&inlined, &index(), &mut engine, false).unwrap();
    runtime.tick().unwrap();
    let counts = take(&calls);
    assert_eq!((counts["mul"], counts["clamp"], counts["below"]), (3, 3, 3));
}

/// `#l = mul({left}, 2)`, `#r = mul({right}, 3)`, `#sum = add(#l, #r)`, read
/// by one leaf.
pub(crate) fn two_branches(store: &SimpleDataStore) -> BehaviorTree {
    build(
        store,
        vec![below(1, read(22), 100.0)],
        vec![
            compute(20, MUL, vec![(MUL_A, key("left")), (MUL_B, f(2.0))], None),
            compute(21, MUL, vec![(MUL_A, key("right")), (MUL_B, f(3.0))], None),
            compute(22, ADD, vec![(ADD_A, read(20)), (ADD_B, read(21))], None),
        ],
    )
}

/// A settled graph makes no calls, and a changed input calls only the nodes
/// downstream of it: here `#l` and `#sum`, not `#r`.
#[test]
fn a_changed_input_calls_only_the_nodes_downstream_of_it() {
    let store = SimpleDataStore::new();
    set(&store, "left", 1.0);
    set(&store, "right", 1.0);
    let tree = two_branches(&store);
    let mut engine = Engine::default();
    let calls = engine.calls.clone();
    let mut runtime = BehaviorTreeRuntime::setup(&tree, &index(), &mut engine, false).unwrap();

    runtime.tick().unwrap();
    assert_eq!(
        take(&calls),
        HashMap::from([("mul", 2), ("add", 1), ("below", 1)])
    );

    runtime.tick().unwrap();
    assert_eq!(
        take(&calls),
        HashMap::from([("below", 1)]),
        "settled: only the tree's leaf calls"
    );

    set(&store, "left", 4.0);
    runtime.tick().unwrap();
    assert_eq!(
        take(&calls),
        HashMap::from([("mul", 1), ("add", 1), ("below", 1)])
    );
    assert_eq!(tree.compute_calls(id(20)), Some(2), "#l: once more");
    assert_eq!(tree.compute_calls(id(21)), Some(1), "#r: not again");
    assert_eq!(tree.compute_calls(id(22)), Some(2), "#sum: once more");
}

/// A node whose result does not change stops the change: its readers keep
/// their cache.
#[test]
fn an_unchanged_result_stops_the_change() {
    let store = SimpleDataStore::new();
    set(&store, "sensor/distance", -0.1);
    let mut compute_nodes = centimetres_nodes();
    compute_nodes.push(compute(
        12,
        ADD,
        vec![(ADD_A, read(11)), (ADD_B, f(1.0))],
        None,
    ));
    let tree = build(&store, vec![below(1, read(12), 30.0)], compute_nodes);
    let mut engine = Engine::default();
    let mut runtime = BehaviorTreeRuntime::setup(&tree, &index(), &mut engine, false).unwrap();

    runtime.tick().unwrap();
    // -20 cm also clamps to 0: `#raw` and `#cm` run again, `#cm` gives the
    // same 0, and `add` does not run.
    set(&store, "sensor/distance", -0.2);
    runtime.tick().unwrap();
    assert_eq!(
        [10, 11, 12].map(|node| tree.compute_calls(id(node)).unwrap()),
        [2, 2, 1]
    );
}

/// The tree reads a node only when the leaf that reads it ticks: the fallback
/// does not reach its second child while the first succeeds, so the node that
/// child reads never runs. A node nothing reads never runs either.
#[test]
fn only_the_nodes_a_ticking_leaf_reads_are_evaluated() {
    let store = SimpleDataStore::new();
    set(&store, "left", 1.0);
    set(&store, "right", 1.0);
    let tree = build(
        &store,
        vec![
            control(1, FALLBACK_FUNCTION_ID, &[2, 3]),
            below(2, read(20), 100.0),
            below(3, read(21), 100.0),
        ],
        vec![
            compute(20, MUL, vec![(MUL_A, key("left")), (MUL_B, f(2.0))], None),
            compute(21, MUL, vec![(MUL_A, key("right")), (MUL_B, f(3.0))], None),
            compute(29, MUL, vec![(MUL_A, key("left")), (MUL_B, f(9.0))], None),
        ],
    );
    assert_eq!(tree.compute_plan().on_demand, [id(20), id(21)]);
    assert_eq!(tree.compute_plan().unused, [id(29)]);

    let mut engine = Engine::default();
    let mut runtime = BehaviorTreeRuntime::setup(&tree, &index(), &mut engine, false).unwrap();
    assert_eq!(runtime.tick().unwrap(), Status::Success);
    assert_eq!(tree.compute_calls(id(20)), Some(1));
    assert_eq!(
        tree.compute_calls(id(21)),
        Some(0),
        "the second child did not tick"
    );
    assert_eq!(tree.compute_calls(id(29)), Some(0), "nothing reads it");
}

/// A node that writes a store key is brought up to date every tick though
/// the tree does not read it, and it writes its key every tick, from its cache
/// when its inputs did not change. A node that writes a tree-local variable
/// and that nothing reads never runs.
#[test]
fn a_rogue_node_writing_a_store_key_runs_every_tick_from_its_cache() {
    let store = SimpleDataStore::new();
    set(&store, "in", 3.0);
    let tree = build(
        &store,
        vec![below(1, f(1.0), 2.0)],
        vec![
            compute(
                30,
                MUL,
                vec![(MUL_A, key("in")), (MUL_B, f(2.0))],
                Some("out"),
            ),
            compute(
                31,
                MUL,
                vec![(MUL_A, key("in")), (MUL_B, f(5.0))],
                Some(".x"),
            ),
        ],
    );
    assert_eq!(tree.compute_plan().every_tick, [id(30)]);
    assert_eq!(tree.compute_plan().unused, [id(31)]);

    let mut engine = Engine::default();
    let mut runtime = BehaviorTreeRuntime::setup(&tree, &index(), &mut engine, false).unwrap();
    runtime.tick().unwrap();
    assert_eq!(get(&store, "out"), Some(Value::F64(6.0)));

    // Someone else writes `out`: the next tick writes it back, from the cache.
    set(&store, "out", -1.0);
    runtime.tick().unwrap();
    assert_eq!(get(&store, "out"), Some(Value::F64(6.0)));
    assert_eq!(
        tree.compute_calls(id(30)),
        Some(1),
        "its input did not change"
    );
    assert_eq!(tree.compute_calls(id(31)), Some(0));

    set(&store, "in", 4.0);
    runtime.tick().unwrap();
    assert_eq!(get(&store, "out"), Some(Value::F64(8.0)));
    assert_eq!(tree.compute_calls(id(30)), Some(2));
}

/// Of two nodes writing one store key, the later one wins at every tick, also
/// when only the earlier one ran again.
#[test]
fn the_later_of_two_nodes_writing_one_key_wins_every_tick() {
    let store = SimpleDataStore::new();
    set(&store, "x", 1.0);
    set(&store, "y", 2.0);
    let tree = build(
        &store,
        vec![below(1, f(1.0), 2.0)],
        vec![
            compute(
                40,
                MUL,
                vec![(MUL_A, key("x")), (MUL_B, f(1.0))],
                Some("out"),
            ),
            compute(
                41,
                MUL,
                vec![(MUL_A, key("y")), (MUL_B, f(1.0))],
                Some("out"),
            ),
        ],
    );
    let mut engine = Engine::default();
    let mut runtime = BehaviorTreeRuntime::setup(&tree, &index(), &mut engine, false).unwrap();
    runtime.tick().unwrap();
    assert_eq!(get(&store, "out"), Some(Value::F64(2.0)));

    set(&store, "x", 7.0);
    runtime.tick().unwrap();
    assert_eq!(get(&store, "out"), Some(Value::F64(2.0)), "y still wins");
    assert_eq!(tree.compute_calls(id(41)), Some(1), "from its cache");
}

/// A volatile node calls its function once per tick, however many leaves read
/// it. A node that is not volatile, with no arguments, would call it once and
/// keep that result: a sensor read as a function is volatile.
#[test]
fn a_volatile_node_calls_its_function_once_per_tick() {
    let store = SimpleDataStore::new();
    let mut sensor = compute(50, DISTANCE, vec![], None);
    sensor.volatile = true;
    let stale = compute(51, DISTANCE, vec![], None);
    let tree = build(
        &store,
        vec![
            control(1, PARALLEL_FUNCTION_ID, &[2, 3, 4]),
            below(2, read(50), 1.0),
            below(3, read(50), 2.0),
            below(4, read(51), 1.0),
        ],
        vec![sensor, stale],
    );
    let mut engine = Engine::default();
    let sensors = engine.sensors.clone();
    let calls = engine.calls.clone();
    let mut runtime = BehaviorTreeRuntime::setup(&tree, &index(), &mut engine, false).unwrap();
    for distance in [0.5, 1.5] {
        sensors.borrow_mut().insert(DISTANCE, distance);
        runtime.tick().unwrap();
        // The last leaf reads the node that is not volatile.
        let stale = calls
            .borrow()
            .iter()
            .rfind(|(name, _)| *name == "below")
            .map(|(_, args)| args[&BELOW_X].clone());
        assert_eq!(stale, Some(Value::F64(0.5)), "it keeps its first reading");
        calls.borrow_mut().clear();
    }
    assert_eq!(
        tree.compute_calls(id(50)),
        Some(2),
        "once per tick, two readers"
    );
    assert_eq!(tree.compute_calls(id(51)), Some(1));
}

/// A stateful node keeps its state in its mutable parameter, starting from the
/// literal. A changed state is a changed input, so it is called until the
/// state settles, then no more. Its state is a port the tree reads.
#[test]
fn a_stateful_node_is_called_until_its_state_settles() {
    let store = SimpleDataStore::new();
    set(&store, "target", 2.0);
    let tree = build(
        &store,
        vec![below(1, port(60, SMOOTH_STATE), 100.0)],
        vec![compute(
            60,
            SMOOTH,
            vec![
                (SMOOTH_X, key("target")),
                (SMOOTH_ALPHA, f(1.0)),
                (SMOOTH_STATE, f(0.0)),
            ],
            Some("out"),
        )],
    );
    let mut engine = Engine::default();
    let calls = engine.calls.clone();
    let mut runtime = BehaviorTreeRuntime::setup(&tree, &index(), &mut engine, false).unwrap();

    let mut smooth_calls = Vec::new();
    for _ in 0..4 {
        runtime.tick().unwrap();
        smooth_calls.push(take(&calls).get("smooth").copied().unwrap_or(0));
    }
    // The first call moves the state from 0 to 2, the second finds it at 2
    // and leaves it there, and from then on the arguments are those of the
    // last call.
    assert_eq!(smooth_calls, [1, 1, 0, 0]);
    assert_eq!(get(&store, "out"), Some(Value::F64(2.0)));

    set(&store, "target", 3.0);
    runtime.tick().unwrap();
    assert_eq!(take(&calls).get("smooth"), Some(&1));
    assert_eq!(get(&store, "out"), Some(Value::F64(3.0)));
}

/// The first read of a node in a tick brings it up to date, with what the
/// tree wrote before that read. A later read in the same tick reuses it,
/// even after the tree wrote again: the write reaches the node next tick.
#[test]
fn a_node_is_brought_up_to_date_at_its_first_read_in_a_tick() {
    let store = SimpleDataStore::new();
    // .v = add(a: .v, b: 1) each time it ticks, from 1.
    let increment = |node_id| {
        node(
            node_id,
            ADD,
            vec![
                (ADD_A, key(".v")),
                (ADD_B, f(1.0)),
                (_RET_PARAM_ID, key(".v")),
            ],
        )
    };
    let tree = build(
        &store,
        vec![
            control(1, SEQ_FUNCTION_ID, &[2, 3, 4, 5]),
            increment(2),
            below(3, read(70), 100.0),
            increment(4),
            below(5, read(70), 100.0),
        ],
        vec![compute(
            70,
            MUL,
            vec![(MUL_A, key(".v")), (MUL_B, f(10.0))],
            None,
        )],
    );
    tree.variables.borrow()[&var(".v")].set(Value::F64(0.0));
    let mut engine = Engine::default();
    let calls = engine.calls.clone();
    let mut runtime = BehaviorTreeRuntime::setup(&tree, &index(), &mut engine, false).unwrap();

    let readings = |calls: &Calls| -> Vec<Value> {
        calls
            .borrow_mut()
            .drain(..)
            .filter(|(name, _)| *name == "below")
            .map(|(_, args)| args[&BELOW_X].clone())
            .collect()
    };
    runtime.tick().unwrap();
    // .v is 1 at the first read, 2 at the second, which reuses the first.
    assert_eq!(readings(&calls), [Value::F64(10.0), Value::F64(10.0)]);
    runtime.tick().unwrap();
    // .v is 3 at this tick's first read.
    assert_eq!(readings(&calls), [Value::F64(30.0), Value::F64(30.0)]);
}

fn build_error(nodes: Vec<Node>, compute: Vec<ComputeNode>) -> String {
    let store = SimpleDataStore::new();
    let resolver = |name: &str| Some(store.slot(&Key::from(name)));
    match load_behavior_tree_with_compute(nodes, compute, &resolver, &names()) {
        Ok(_) => panic!("the tree builds"),
        Err(error) => error.to_string(),
    }
}

/// Building refuses a cycle, a reference to no compute node, and a compute
/// node reading a tree node's argument.
#[test]
fn building_refuses_what_a_graph_cannot_evaluate() {
    let tree = || vec![below(1, read(80), 1.0)];
    let error = build_error(
        tree(),
        vec![
            compute(80, ADD, vec![(ADD_A, read(81)), (ADD_B, f(1.0))], None),
            compute(81, ADD, vec![(ADD_A, read(80)), (ADD_B, f(1.0))], None),
        ],
    );
    assert!(error.contains("in a cycle"), "{error}");

    let error = build_error(tree(), Vec::new());
    assert!(error.contains("is not a compute node"), "{error}");

    let error = build_error(
        tree(),
        vec![compute(
            80,
            ADD,
            vec![(
                ADD_A,
                Expression::NodeArgument(NodeParameterId {
                    node: id(1),
                    parameter: BELOW_LIMIT,
                }),
            )],
            None,
        )],
    );
    assert!(error.contains("reads a tree node's argument"), "{error}");
}

fn lowering_error(nodes: Vec<Node>, compute: Vec<ComputeNode>) -> String {
    let store = SimpleDataStore::new();
    let tree = build(&store, nodes, compute);
    let mut engine = Engine::default();
    match BehaviorTreeRuntime::setup(&tree, &index(), &mut engine, false) {
        Ok(_) => panic!("the tree lowers"),
        Err(error) => error.to_string(),
    }
}

/// Lowering refuses a port that is neither a return value nor a mutable
/// parameter, and a mutable parameter fed by another node's port.
#[test]
fn lowering_refuses_a_port_that_is_no_output() {
    let error = lowering_error(
        vec![below(1, port(90, MUL_A), 1.0)],
        vec![compute(
            90,
            MUL,
            vec![(MUL_A, f(1.0)), (MUL_B, f(1.0))],
            None,
        )],
    );
    assert!(error.contains("mul has no such output"), "{error}");

    let error = lowering_error(
        vec![below(1, read(91), 1.0)],
        vec![
            compute(90, MUL, vec![(MUL_A, f(1.0)), (MUL_B, f(1.0))], None),
            compute(
                91,
                SMOOTH,
                vec![
                    (SMOOTH_X, f(1.0)),
                    (SMOOTH_ALPHA, f(1.0)),
                    (SMOOTH_STATE, read(90)),
                ],
                None,
            ),
        ],
    );
    assert!(error.contains("mutable parameter `state`"), "{error}");
}

/// The text form of a graph: one line per node, inputs before their readers,
/// with the store key a node writes after it.
#[test]
fn a_graph_shows_one_node_per_line() {
    let store = SimpleDataStore::new();
    let mut nodes = centimetres_nodes();
    nodes.push(compute(
        12,
        ADD,
        vec![(ADD_A, read(11)), (ADD_B, f(1.0))],
        Some("face/level"),
    ));
    // Given readers first: the display follows the evaluation order.
    nodes.reverse();
    let tree = build(&store, vec![below(1, read(11), 30.0)], nodes);
    let functions = index();
    let variables = names();
    let compute_names = HashMap::from([
        (id(10), "raw".to_string()),
        (id(11), "cm".to_string()),
        (id(12), "level".to_string()),
    ]);
    let names = Names {
        functions: Some(&functions),
        variables: Some(&variables),
        compute: Some(&compute_names),
    };
    assert_eq!(
        tree.display_compute(names),
        "#raw = mul(a: {sensor/distance}, b: 100f64)\n\
         #cm = clamp(x: #raw, lo: 0f64)\n\
         #level = add(a: #cm, b: 1f64) -> {face/level}\n"
    );
    assert_eq!(
        tree.node_index[&id(1)].display(names).to_string(),
        "below(x: #cm, limit: 30f64)"
    );
}

/// The optimization, counted: the cascade of
/// [`a_node_read_by_several_leaves_calls_its_function_once_per_tick`] over a
/// hundred ticks, the distance changing every tenth tick. Written as call
/// expressions, every leaf calls the whole cascade every tick. Written as
/// compute nodes, the cascade runs once per change.
#[test]
fn over_a_hundred_ticks_the_cascade_runs_once_per_change() {
    let store = SimpleDataStore::new();
    let as_nodes = build(&store, three_readers(|| read(11)), centimetres_nodes());
    let as_calls = build(&store, three_readers(centimetres), Vec::new());

    let mut totals = Vec::new();
    for tree in [&as_calls, &as_nodes] {
        let mut engine = Engine::default();
        let sensors = engine.sensors.clone();
        let calls = engine.calls.clone();
        let mut runtime = BehaviorTreeRuntime::setup(tree, &index(), &mut engine, false).unwrap();
        let mut total: HashMap<&'static str, usize> = HashMap::new();
        for tick in 0..100 {
            let distance = 0.1 * (tick / 10) as f64;
            set(&store, "sensor/distance", distance);
            sensors.borrow_mut().insert(DISTANCE, distance);
            runtime.tick().unwrap();
            for (name, count) in take(&calls) {
                *total.entry(name).or_insert(0) += count;
            }
        }
        totals.push(total);
    }
    let row = |total: &HashMap<&str, usize>| {
        ["distance", "mul", "clamp", "below"].map(|name| total.get(name).copied().unwrap_or(0))
    };
    eprintln!("calls over 100 ticks: [distance, mul, clamp, below]");
    eprintln!("  call expressions: {:?}", row(&totals[0]));
    eprintln!("  compute nodes:    {:?}", row(&totals[1]));
    assert_eq!(row(&totals[0]), [300, 300, 300, 300]);
    assert_eq!(row(&totals[1]), [0, 10, 10, 300]);
}
