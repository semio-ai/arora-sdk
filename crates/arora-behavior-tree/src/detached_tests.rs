//! Detached nodes: the calls of a node graph listed with the tree but held by
//! none of its nodes, evaluated when what they output is read, once per tick,
//! and only when their inputs changed.
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
    call, centimetres, control, f, index, node, Calls, Engine, ADD, ADD_A, ADD_B, BELOW,
    BELOW_LIMIT, BELOW_X, CLAMP, CLAMP_LO, CLAMP_X, DISTANCE, MUL, MUL_A, MUL_B, SMOOTH,
    SMOOTH_ALPHA, SMOOTH_STATE, SMOOTH_X,
};
use crate::display::Names;
use crate::nodes::{
    FALLBACK_FUNCTION_ID, PARALLEL_FUNCTION_ID, SEQ_FUNCTION_ID, UNPACK_FUNCTION_ID,
    UNPACK_OUT_PARAM_IDS, UNPACK_VALUES_PARAM_ID,
};
use crate::schema::{Expression, Node, NodeParameterId, _RET_PARAM_ID};
use crate::{load_behavior_tree_nodes_with, BehaviorTree, BehaviorTreeRuntime};
use arora_behavior::Status;

/// The variable id of the store key or tree-local name `name`.
fn var(name: &str) -> Uuid {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    name.hash(&mut hasher);
    Uuid::from_u128(u128::from(hasher.finish()) << 64 | 0x7A12)
}

/// The variable `name`: a store key, or tree-local when it starts with `.`.
fn key(name: &str) -> Expression {
    Expression::VariableId(var(name))
}

fn id(node: u128) -> Uuid {
    Uuid::from_u128(node)
}

/// `@node`: a node's return value.
fn read(node: u128) -> Expression {
    port(node, _RET_PARAM_ID)
}

/// `@node.parameter`: a node's argument, here a mutable parameter's value
/// after the node's last call.
fn port(node: u128, parameter: Uuid) -> Expression {
    Expression::NodeArgument(NodeParameterId {
        node: id(node),
        parameter,
    })
}

/// A node of the graph: a node like any other, with its return value bound
/// to `output` when given. Listed after the tree, no node holds it.
fn compute(
    node: u128,
    function: Uuid,
    inputs: Vec<(Uuid, Expression)>,
    output: Option<&str>,
) -> Node {
    let mut arguments: HashMap<Uuid, Expression> = inputs.into_iter().collect();
    if let Some(output) = output {
        arguments.insert(_RET_PARAM_ID, key(output));
    }
    Node {
        id: id(node),
        function,
        arguments,
        children: None,
    }
}

/// `function(…)` whose parameter ids are expressions of their own.
fn raw_call(function: Uuid, arguments: Vec<(Expression, Expression)>) -> Expression {
    let Expression::Call(mut stated) = call(function, Vec::new()) else {
        unreachable!("call builds a call");
    };
    stated.arguments = arguments;
    Expression::Call(stated)
}

/// `below(x, limit: limit)`, a leaf of the tree.
fn below(node_id: u128, x: Expression, limit: f64) -> Node {
    node(node_id, BELOW, vec![(BELOW_X, x), (BELOW_LIMIT, f(limit))])
}

/// The names of every variable the tests use, for the resolver and display.
const NAMES: &[&str] = &[
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

fn names() -> HashMap<Uuid, String> {
    NAMES
        .iter()
        .map(|name| (var(name), name.to_string()))
        .collect()
}

/// Build a tree from `nodes`, the root first, and `graph` after them, whose
/// variables are the store's keys, but for the `.` ones.
fn build(store: &SimpleDataStore, nodes: Vec<Node>, graph: Vec<Node>) -> BehaviorTree {
    let resolver = |name: &str| Some(store.slot(&Key::from(name)));
    let listed = nodes.into_iter().chain(graph).collect();
    load_behavior_tree_nodes_with(listed, &resolver, &names()).expect("the tree builds")
}

fn set(store: &SimpleDataStore, name: &str, value: f64) {
    store
        .write(StateChange::set(name, Value::F64(value)))
        .expect("the store writes");
}

fn get(store: &SimpleDataStore, name: &str) -> Option<Value> {
    store.read(&[Key::from(name)]).remove(0)
}

/// How many calls each function received since the last time this was asked.
fn take(calls: &Calls) -> HashMap<&'static str, usize> {
    let mut counts = HashMap::new();
    for (name, _) in calls.borrow_mut().drain(..) {
        *counts.entry(name).or_insert(0) += 1;
    }
    counts
}

/// `raw: mul(a: {sensor/distance}, b: 100)`, `cm: clamp(x: @raw, lo: 0)`:
/// the distance in centimetres, never negative.
fn centimetres_nodes() -> Vec<Node> {
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
fn three_readers(x: impl Fn() -> Expression) -> Vec<Node> {
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

/// `l: mul({left}, 2)`, `r: mul({right}, 3)`, `sum: add(@l, @r)`, read by
/// one leaf.
fn two_branches(store: &SimpleDataStore) -> BehaviorTree {
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
    assert_eq!(tree.detached_calls(id(20)), Some(2), "#l: once more");
    assert_eq!(tree.detached_calls(id(21)), Some(1), "#r: not again");
    assert_eq!(tree.detached_calls(id(22)), Some(2), "#sum: once more");
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
        [10, 11, 12].map(|node| tree.detached_calls(id(node)).unwrap()),
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
    let mut engine = Engine::default();
    let mut runtime = BehaviorTreeRuntime::setup(&tree, &index(), &mut engine, false).unwrap();
    assert_eq!(tree.detached_plan().on_demand, [id(20), id(21)]);
    assert_eq!(tree.detached_plan().unused, [id(29)]);
    assert_eq!(runtime.tick().unwrap(), Status::Success);
    assert_eq!(tree.detached_calls(id(20)), Some(1));
    assert_eq!(
        tree.detached_calls(id(21)),
        Some(0),
        "the second child did not tick"
    );
    assert_eq!(tree.detached_calls(id(29)), Some(0), "nothing reads it");
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
    let mut engine = Engine::default();
    let mut runtime = BehaviorTreeRuntime::setup(&tree, &index(), &mut engine, false).unwrap();
    assert_eq!(tree.detached_plan().every_tick, [id(30)]);
    assert_eq!(tree.detached_plan().unused, [id(31)]);
    runtime.tick().unwrap();
    assert_eq!(get(&store, "out"), Some(Value::F64(6.0)));

    // Someone else writes `out`: the next tick writes it back, from the cache.
    set(&store, "out", -1.0);
    runtime.tick().unwrap();
    assert_eq!(get(&store, "out"), Some(Value::F64(6.0)));
    assert_eq!(
        tree.detached_calls(id(30)),
        Some(1),
        "its input did not change"
    );
    assert_eq!(tree.detached_calls(id(31)), Some(0));

    set(&store, "in", 4.0);
    runtime.tick().unwrap();
    assert_eq!(get(&store, "out"), Some(Value::F64(8.0)));
    assert_eq!(tree.detached_calls(id(30)), Some(2));
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
    assert_eq!(tree.detached_calls(id(41)), Some(1), "from its cache");
}

/// A node is called when its arguments change, however its function gets its
/// result. A value that changes at every tick, such as the clock, calls the
/// node that reads it at every tick, once however many leaves read it. A
/// function whose result follows from no argument, such as a sensor read as a
/// function, is called once and keeps that result: what it depends on is an
/// argument, or a store key its source writes.
#[test]
fn a_node_reading_the_clock_is_called_once_per_tick() {
    let store = SimpleDataStore::new();
    let tree = build(
        &store,
        vec![
            control(1, PARALLEL_FUNCTION_ID, &[2, 3, 4]),
            below(2, read(50), 1.0),
            below(3, read(50), 2.0),
            below(4, read(51), 1.0),
        ],
        vec![
            compute(50, ADD, vec![(ADD_A, key("x")), (ADD_B, f(0.0))], None),
            compute(51, DISTANCE, vec![], None),
        ],
    );
    let mut engine = Engine::default();
    let sensors = engine.sensors.clone();
    let calls = engine.calls.clone();
    let mut runtime = BehaviorTreeRuntime::setup(&tree, &index(), &mut engine, false).unwrap();
    for (tick, distance) in [(1.0, 0.5), (2.0, 1.5)] {
        set(&store, "x", tick);
        sensors.borrow_mut().insert(DISTANCE, distance);
        runtime.tick().unwrap();
        // The last leaf reads the sensor called with no argument.
        let stale = calls
            .borrow()
            .iter()
            .rfind(|(name, _)| *name == "below")
            .map(|(_, args)| args[&BELOW_X].clone());
        assert_eq!(stale, Some(Value::F64(0.5)), "it keeps its first reading");
        calls.borrow_mut().clear();
    }
    assert_eq!(
        tree.detached_calls(id(50)),
        Some(2),
        "once per tick, two readers"
    );
    assert_eq!(tree.detached_calls(id(51)), Some(1));
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

fn build_error(nodes: Vec<Node>, graph: Vec<Node>) -> String {
    let store = SimpleDataStore::new();
    let resolver = |name: &str| Some(store.slot(&Key::from(name)));
    let listed = nodes.into_iter().chain(graph).collect();
    match load_behavior_tree_nodes_with(listed, &resolver, &names()) {
        Ok(_) => panic!("the tree builds"),
        Err(error) => error.to_string(),
    }
}

/// Building refuses a reference to no node.
#[test]
fn building_refuses_a_reference_to_no_node() {
    let error = build_error(vec![below(1, read(80), 1.0)], Vec::new());
    assert!(error.contains("was not found"), "{error}");
}

/// A node no node holds that is not a call to a module function, such as a
/// control node an edit adds before placing it, or a node with children,
/// waits until a node holds it: it is not detached, and never runs.
#[test]
fn a_loose_node_that_is_no_call_waits_until_a_node_holds_it() {
    let store = SimpleDataStore::new();
    let mut parent = compute(81, ADD, vec![(ADD_A, f(1.0)), (ADD_B, f(1.0))], None);
    parent.children = Some(vec![id(1)]);
    let tree = build(
        &store,
        vec![below(1, f(1.0), 2.0)],
        vec![control(80, SEQ_FUNCTION_ID, &[]), parent],
    );
    let mut engine = Engine::default();
    let calls = engine.calls.clone();
    let mut runtime = BehaviorTreeRuntime::setup(&tree, &index(), &mut engine, false).unwrap();
    runtime.tick().unwrap();
    assert_eq!(tree.detached_plan(), Default::default());
    assert_eq!(take(&calls), HashMap::from([("below", 1)]));
}

fn lowering_error(nodes: Vec<Node>, graph: Vec<Node>) -> String {
    let store = SimpleDataStore::new();
    let tree = build(&store, nodes, graph);
    let mut engine = Engine::default();
    match BehaviorTreeRuntime::setup(&tree, &index(), &mut engine, false) {
        Ok(_) => panic!("the tree lowers"),
        Err(error) => error.to_string(),
    }
}

/// Lowering refuses detached nodes that read each other in a cycle, a read of
/// a port that is neither a return value nor a mutable parameter, and a cell
/// a detached node writes that another node writes too: here a mutable
/// parameter fed by another node's return value.
#[test]
fn lowering_refuses_what_a_graph_cannot_evaluate() {
    let error = lowering_error(
        vec![below(1, read(80), 1.0)],
        vec![
            compute(80, ADD, vec![(ADD_A, read(81)), (ADD_B, f(1.0))], None),
            compute(81, ADD, vec![(ADD_A, read(80)), (ADD_B, f(1.0))], None),
        ],
    );
    assert!(error.contains("in a cycle"), "{error}");

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
    assert!(error.contains("one tree-local cell"), "{error}");
}

/// A detached node reads the tree as any node does: here a leaf's argument.
#[test]
fn a_detached_node_reads_a_tree_node_s_argument() {
    let store = SimpleDataStore::new();
    let tree = build(
        &store,
        vec![
            control(1, SEQ_FUNCTION_ID, &[2, 3]),
            below(2, f(1.0), 4.0),
            below(3, read(80), 100.0),
        ],
        vec![compute(
            80,
            MUL,
            vec![(MUL_A, port(2, BELOW_LIMIT)), (MUL_B, f(10.0))],
            None,
        )],
    );
    let mut engine = Engine::default();
    let calls = engine.calls.clone();
    let mut runtime = BehaviorTreeRuntime::setup(&tree, &index(), &mut engine, false).unwrap();
    runtime.tick().unwrap();
    let reading = calls
        .borrow()
        .iter()
        .rfind(|(name, _)| *name == "below")
        .map(|(_, args)| args[&BELOW_X].clone());
    assert_eq!(reading, Some(Value::F64(40.0)));
}

/// Every port of a node is an output: here the state of `smooth`, a mutable
/// parameter, written to a store key while the return value goes to another.
#[test]
fn a_node_writes_each_of_its_outputs() {
    let store = SimpleDataStore::new();
    set(&store, "target", 2.0);
    let smooth = compute(
        60,
        SMOOTH,
        vec![
            (SMOOTH_X, key("target")),
            (SMOOTH_ALPHA, f(0.5)),
            (SMOOTH_STATE, key("y")),
        ],
        Some("out"),
    );
    set(&store, "y", 0.0);
    let tree = build(&store, vec![below(1, f(1.0), 2.0)], vec![smooth]);
    let mut engine = Engine::default();
    let mut runtime = BehaviorTreeRuntime::setup(&tree, &index(), &mut engine, false).unwrap();
    assert_eq!(tree.detached_plan().every_tick, [id(60)]);
    runtime.tick().unwrap();
    assert_eq!(
        get(&store, "y"),
        Some(Value::F64(1.0)),
        "the state, halfway"
    );
    assert_eq!(
        get(&store, "out"),
        Some(Value::F64(1.0)),
        "the return value"
    );
}

/// A sink writes its store keys after the tree has ticked, even when the
/// tree brings it up to date earlier: within a tick, the tree reads the store
/// as it was, and the key at the next tick.
#[test]
fn a_sink_writes_its_keys_after_the_tree_ticks() {
    let store = SimpleDataStore::new();
    set(&store, "target", 2.0);
    set(&store, "out", -1.0);
    let tree = build(
        &store,
        vec![
            control(1, SEQ_FUNCTION_ID, &[2, 3]),
            // Reading the state brings the sink up to date during the tick.
            below(2, port(60, SMOOTH_STATE), 100.0),
            below(3, key("out"), 100.0),
        ],
        vec![compute(
            60,
            SMOOTH,
            vec![
                (SMOOTH_X, key("target")),
                (SMOOTH_ALPHA, f(0.5)),
                (SMOOTH_STATE, f(0.0)),
            ],
            Some("out"),
        )],
    );
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
    assert_eq!(readings(&calls), [Value::F64(1.0), Value::F64(-1.0)]);
    assert_eq!(get(&store, "out"), Some(Value::F64(1.0)), "written after");
    runtime.tick().unwrap();
    assert_eq!(readings(&calls), [Value::F64(1.5), Value::F64(1.0)]);
}

/// Sinks are all brought up to date before any publishes its keys: two
/// graphs feeding each other through store keys each read the other's value
/// of the previous tick, whatever their order.
#[test]
fn sinks_publish_after_every_sink_is_up_to_date() {
    let store = SimpleDataStore::new();
    set(&store, "x", 0.0);
    set(&store, "y", 0.0);
    let tree = build(
        &store,
        vec![below(1, f(1.0), 2.0)],
        vec![
            compute(30, ADD, vec![(ADD_A, key("y")), (ADD_B, f(1.0))], Some("x")),
            compute(31, ADD, vec![(ADD_A, key("x")), (ADD_B, f(1.0))], Some("y")),
        ],
    );
    let mut engine = Engine::default();
    let mut runtime = BehaviorTreeRuntime::setup(&tree, &index(), &mut engine, false).unwrap();
    runtime.tick().unwrap();
    assert_eq!(
        (get(&store, "x"), get(&store, "y")),
        (Some(Value::F64(1.0)), Some(Value::F64(1.0)))
    );
    runtime.tick().unwrap();
    assert_eq!(
        (get(&store, "x"), get(&store, "y")),
        (Some(Value::F64(2.0)), Some(Value::F64(2.0)))
    );
}

/// A port bound to a store key keeps a tree-local cell: `@node` reads the
/// node's current result, while the key reads as the store holds it.
#[test]
fn a_port_bound_to_a_store_key_reads_its_current_value() {
    let store = SimpleDataStore::new();
    set(&store, "in", 3.0);
    set(&store, "out", -1.0);
    let tree = build(
        &store,
        vec![
            control(1, SEQ_FUNCTION_ID, &[2, 3]),
            below(2, read(30), 100.0),
            below(3, key("out"), 100.0),
        ],
        vec![compute(
            30,
            MUL,
            vec![(MUL_A, key("in")), (MUL_B, f(2.0))],
            Some("out"),
        )],
    );
    let mut engine = Engine::default();
    let calls = engine.calls.clone();
    let mut runtime = BehaviorTreeRuntime::setup(&tree, &index(), &mut engine, false).unwrap();
    runtime.tick().unwrap();
    let readings: Vec<Value> = calls
        .borrow()
        .iter()
        .filter(|(name, _)| *name == "below")
        .map(|(_, args)| args[&BELOW_X].clone())
        .collect();
    assert_eq!(readings, [Value::F64(6.0), Value::F64(-1.0)]);
    assert_eq!(get(&store, "out"), Some(Value::F64(6.0)));
}

/// Sinks publish in the order the tree lists them, not in the order they are
/// evaluated: here the earlier-listed sink reads the later one's state, and the
/// later one still wins their shared key.
#[test]
fn the_later_listed_sink_wins_whatever_reads_what() {
    let store = SimpleDataStore::new();
    set(&store, "target", 2.0);
    let tree = build(
        &store,
        vec![below(1, f(1.0), 2.0)],
        vec![
            compute(
                40,
                ADD,
                vec![(ADD_A, port(41, SMOOTH_STATE)), (ADD_B, f(100.0))],
                Some("out"),
            ),
            compute(
                41,
                SMOOTH,
                vec![
                    (SMOOTH_X, key("target")),
                    (SMOOTH_ALPHA, f(1.0)),
                    (SMOOTH_STATE, f(0.0)),
                ],
                Some("out"),
            ),
        ],
    );
    let mut engine = Engine::default();
    let mut runtime = BehaviorTreeRuntime::setup(&tree, &index(), &mut engine, false).unwrap();
    runtime.tick().unwrap();
    assert_eq!(get(&store, "out"), Some(Value::F64(2.0)), "41, listed last");
}

/// A tree-local variable that only a nested call names, such as its state,
/// is carried over when the tree is rebuilt, as any variable its nodes read.
#[test]
fn a_rebuilt_tree_keeps_the_variables_its_calls_name() {
    let store = SimpleDataStore::new();
    set(&store, "target", 2.0);
    let nodes = || {
        vec![below(
            1,
            call(
                SMOOTH,
                vec![
                    (SMOOTH_X, key("target")),
                    (SMOOTH_ALPHA, f(0.5)),
                    (SMOOTH_STATE, key(".x")),
                ],
            ),
            100.0,
        )]
    };
    let before = build(&store, nodes(), Vec::new());
    before.variables.borrow()[&var(".x")].set(Value::F64(0.0));
    let mut engine = Engine::default();
    let mut runtime = BehaviorTreeRuntime::setup(&before, &index(), &mut engine, false).unwrap();
    runtime.tick().unwrap();
    assert_eq!(
        before.variables.borrow()[&var(".x")].get(),
        Some(Value::F64(1.0))
    );

    let after = build(&store, nodes(), Vec::new());
    after.keep_state(&before, &std::collections::HashSet::new());
    assert_eq!(
        after.variables.borrow()[&var(".x")].get(),
        Some(Value::F64(1.0))
    );
}

/// A uuid literal argument of a nested call is the call site's own cell, as
/// any literal is, so a function that mutates it keeps what it wrote.
#[test]
fn a_uuid_literal_of_a_call_is_its_own_cell() {
    let argument = Expression::Uuid(id(7));
    let cells = crate::with_call_site_cells(call(ADD, vec![(ADD_A, argument)]));
    let Expression::Call(call) = cells else {
        panic!("a call");
    };
    match &call.arguments[0].1 {
        Expression::Variable(cell) => {
            assert_eq!(cell.get(), Some(Value::ArrayU8(id(7).as_bytes().to_vec())))
        }
        other => panic!("a cell, not {other:?}"),
    }
}

/// A cell a detached node writes has that one writer: lowering refuses a
/// nested call that writes it back, and an `Unpack` output bound to it.
#[test]
fn lowering_refuses_a_second_writer_of_a_detached_output() {
    let detached = || compute(50, ADD, vec![(ADD_A, f(1.0)), (ADD_B, f(1.0))], Some(".v"));
    let error = lowering_error(
        vec![below(
            1,
            call(
                SMOOTH,
                vec![
                    (SMOOTH_X, f(1.0)),
                    (SMOOTH_ALPHA, f(1.0)),
                    (SMOOTH_STATE, key(".v")),
                ],
            ),
            100.0,
        )],
        vec![detached()],
    );
    assert!(error.contains("one tree-local cell"), "{error}");

    let error = lowering_error(
        vec![node(
            1,
            UNPACK_FUNCTION_ID,
            vec![
                (UNPACK_VALUES_PARAM_ID, key("in")),
                (UNPACK_OUT_PARAM_IDS[0], key(".v")),
            ],
        )],
        vec![detached()],
    );
    assert!(error.contains("one tree-local cell"), "{error}");

    // A detached node's state bound to an argument a tree node computes from
    // a call: the tree node writes the call's result there.
    let error = lowering_error(
        vec![below(
            1,
            call(ADD, vec![(ADD_A, f(1.0)), (ADD_B, f(1.0))]),
            100.0,
        )],
        vec![compute(
            50,
            SMOOTH,
            vec![
                (SMOOTH_X, f(1.0)),
                (SMOOTH_ALPHA, f(1.0)),
                (SMOOTH_STATE, port(1, BELOW_X)),
            ],
            None,
        )],
    );
    assert!(error.contains("one tree-local cell"), "{error}");

    // A call that computes a parameter id, and writes the output back.
    let smooth_v = || {
        call(
            SMOOTH,
            vec![
                (SMOOTH_X, f(1.0)),
                (SMOOTH_ALPHA, f(1.0)),
                (SMOOTH_STATE, key(".v")),
            ],
        )
    };
    let error = lowering_error(
        vec![below(
            1,
            raw_call(
                ADD,
                vec![(smooth_v(), f(1.0)), (Expression::Uuid(ADD_B), f(1.0))],
            ),
            100.0,
        )],
        vec![detached()],
    );
    assert!(error.contains("one tree-local cell"), "{error}");

    // A call whose parameter id is computed may write back any cell it names.
    let error = lowering_error(
        vec![below(
            1,
            raw_call(
                SMOOTH,
                vec![
                    (Expression::Uuid(SMOOTH_X), f(1.0)),
                    (Expression::Uuid(SMOOTH_ALPHA), f(1.0)),
                    (key(".p"), key(".v")),
                ],
            ),
            100.0,
        )],
        vec![detached()],
    );
    assert!(error.contains("one tree-local cell"), "{error}");
}

/// A call in a detached node's arguments writes back no store key: the node
/// writes the store only through its own outputs, after the tree ticks.
#[test]
fn a_call_in_a_detached_node_writes_back_no_store_key() {
    let error = lowering_error(
        vec![below(1, read(50), 100.0)],
        vec![compute(
            50,
            ADD,
            vec![
                (
                    ADD_A,
                    call(
                        SMOOTH,
                        vec![
                            (SMOOTH_X, f(1.0)),
                            (SMOOTH_ALPHA, f(1.0)),
                            (SMOOTH_STATE, key("y")),
                        ],
                    ),
                ),
                (ADD_B, f(1.0)),
            ],
            None,
        )],
    );
    assert!(error.contains("writes a store key back"), "{error}");
}

/// A detached node reads its own state, but any other output of its own it
/// reads is feedback, which goes through a store key: lowering refuses it.
#[test]
fn a_detached_node_reading_its_own_output_is_refused() {
    for graph in [
        compute(50, ADD, vec![(ADD_A, read(50)), (ADD_B, f(1.0))], None),
        compute(
            50,
            ADD,
            vec![(ADD_A, key(".v")), (ADD_B, f(1.0))],
            Some(".v"),
        ),
    ] {
        let error = lowering_error(vec![below(1, read(50), 100.0)], vec![graph]);
        assert!(error.contains("reads its own output"), "{error}");
    }

    // Its state, named by a variable, is no feedback.
    let store = SimpleDataStore::new();
    set(&store, "target", 2.0);
    let smooth = compute(
        50,
        SMOOTH,
        vec![
            (SMOOTH_X, key("target")),
            (SMOOTH_ALPHA, f(0.5)),
            (SMOOTH_STATE, key(".x")),
        ],
        None,
    );
    let tree = build(&store, vec![below(1, read(50), 100.0)], vec![smooth]);
    tree.variables.borrow()[&var(".x")].set(Value::F64(0.0));
    let mut engine = Engine::default();
    let mut runtime =
        BehaviorTreeRuntime::setup(&tree, &index(), &mut engine, false).expect("it lowers");
    runtime.tick().expect("it ticks");
    assert_eq!(
        tree.variables.borrow()[&var(".x")].get(),
        Some(Value::F64(1.0))
    );
}

/// A call that computes a parameter id keeps a call-site state of its own,
/// which a rebuilt tree takes over; a literal id stays as it is.
#[test]
fn a_call_computing_an_id_keeps_its_call_site_state() {
    let nodes = || {
        vec![below(
            1,
            raw_call(
                ADD,
                vec![
                    (
                        call(
                            SMOOTH,
                            vec![
                                (SMOOTH_X, f(1.0)),
                                (SMOOTH_ALPHA, f(1.0)),
                                (SMOOTH_STATE, f(0.0)),
                            ],
                        ),
                        f(1.0),
                    ),
                    (Expression::Uuid(ADD_B), f(1.0)),
                ],
            ),
            100.0,
        )]
    };
    let state = |tree: &BehaviorTree| {
        let Expression::Call(outer) = &tree.node_index[&id(1)].arguments[&BELOW_X] else {
            panic!("a call");
        };
        assert_eq!(
            outer.arguments[1].0,
            Expression::Uuid(ADD_B),
            "a literal id"
        );
        let Expression::Call(inner) = &outer.arguments[0].0 else {
            panic!("a call computes the id");
        };
        match &inner.arguments[2].1 {
            Expression::Variable(cell) => cell.clone(),
            other => panic!("a cell, not {other:?}"),
        }
    };
    let store = SimpleDataStore::new();
    let before = build(&store, nodes(), Vec::new());
    state(&before).set(Value::F64(0.5));
    let after = build(&store, nodes(), Vec::new());
    after.keep_state(&before, &std::collections::HashSet::new());
    assert_eq!(state(&after).get(), Some(Value::F64(0.5)));
}

/// Attached and detached nodes show alike: one line per node, its name in
/// front when it has one, its bound return before `=`, and `@node` for
/// another node's return value. The graph shows in the order it is listed.
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
    let tree = build(&store, vec![below(1, read(11), 30.0)], nodes);
    let functions = index();
    let variables = names();
    let node_names = HashMap::from([
        (id(10), "raw".to_string()),
        (id(11), "cm".to_string()),
        (id(12), "level".to_string()),
    ]);
    let names = Names {
        functions: Some(&functions),
        variables: Some(&variables),
        nodes: Some(&node_names),
    };
    assert_eq!(
        tree.display_detached(names),
        "raw: mul(a: {sensor/distance}, b: 100f64)\n\
         cm: clamp(x: @raw, lo: 0f64)\n\
         level: {face/level} = add(a: @cm, b: 1f64)\n"
    );
    assert_eq!(
        tree.node_index[&id(1)].display(names).to_string(),
        "below(x: @cm, limit: 30f64)"
    );
}

/// The optimization, counted: the cascade of
/// [`a_node_read_by_several_leaves_calls_its_function_once_per_tick`] over a
/// hundred ticks, the distance changing every tenth tick. Written as call
/// expressions, every leaf calls the whole cascade every tick. Written as
/// detached nodes, the cascade runs once per change.
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
    eprintln!("  detached nodes:   {:?}", row(&totals[1]));
    assert_eq!(row(&totals[0]), [300, 300, 300, 300]);
    assert_eq!(row(&totals[1]), [0, 10, 10, 300]);
}

/// The sinks run after the tree whether or not it failed: a failing leaf stops
/// no data flow, and the tick reports its error.
#[test]
fn sinks_run_when_the_tree_fails() {
    let store = SimpleDataStore::new();
    let tree = build(
        &store,
        vec![node(1, Uuid::from_u128(0xBAD), vec![])],
        vec![compute(
            50,
            ADD,
            vec![(ADD_A, f(1.0)), (ADD_B, f(1.0))],
            Some("out"),
        )],
    );
    let mut engine = Engine::default();
    let mut runtime =
        BehaviorTreeRuntime::setup(&tree, &index(), &mut engine, false).expect("it lowers");
    assert!(
        runtime.tick().is_err(),
        "the leaf's function does not exist"
    );
    assert_eq!(get(&store, "out"), Some(Value::F64(2.0)));
}

/// An output bound to another node's input feeds it: the reader pulls the
/// detached node, whose result replaces the input's literal.
#[test]
fn an_output_bound_to_an_input_feeds_its_reader() {
    let store = SimpleDataStore::new();
    let mut add = compute(50, ADD, vec![(ADD_A, f(1.0)), (ADD_B, f(1.0))], None);
    add.arguments.insert(_RET_PARAM_ID, port(1, BELOW_X));
    // below(x: 5, limit: 3) fails; with x = add(1, 1) it holds.
    let tree = build(&store, vec![below(1, f(5.0), 3.0)], vec![add]);
    let mut engine = Engine::default();
    let mut runtime =
        BehaviorTreeRuntime::setup(&tree, &index(), &mut engine, false).expect("it lowers");
    assert_eq!(runtime.tick().expect("it ticks"), Status::Success);
    assert_eq!(tree.detached_plan().on_demand, vec![id(50)]);
}

/// A native node reads an input an output is bound to: `Equal` compares the
/// detached node's result, not its argument's literal.
#[test]
fn a_native_reader_reads_the_output_bound_to_its_input() {
    use crate::nodes::{EQUAL_A_PARAM_ID, EQUAL_B_PARAM_ID, EQUAL_FUNCTION_ID};
    let store = SimpleDataStore::new();
    let mut add = compute(50, ADD, vec![(ADD_A, f(1.0)), (ADD_B, f(1.0))], None);
    add.arguments
        .insert(_RET_PARAM_ID, port(1, EQUAL_A_PARAM_ID));
    // Equal(a: 5, b: 2) fails; with a = add(1, 1) it holds.
    let equal = node(
        1,
        EQUAL_FUNCTION_ID,
        vec![(EQUAL_A_PARAM_ID, f(5.0)), (EQUAL_B_PARAM_ID, f(2.0))],
    );
    let tree = build(&store, vec![equal], vec![add]);
    let mut engine = Engine::default();
    let mut runtime =
        BehaviorTreeRuntime::setup(&tree, &index(), &mut engine, false).expect("it lowers");
    assert_eq!(runtime.tick().expect("it ticks"), Status::Success);
}

/// An output bound to a detached node's input feeds it as it feeds a tree
/// node: the binding is a write, not a read of a port.
#[test]
fn an_output_bound_to_a_detached_input_feeds_it() {
    let store = SimpleDataStore::new();
    let mut add = compute(50, ADD, vec![(ADD_A, f(1.0)), (ADD_B, f(1.0))], None);
    add.arguments.insert(_RET_PARAM_ID, port(51, ADD_A));
    let sink = compute(51, ADD, vec![(ADD_A, f(5.0)), (ADD_B, f(1.0))], Some("out"));
    let tree = build(&store, vec![below(1, f(1.0), 3.0)], vec![sink, add]);
    let mut engine = Engine::default();
    let mut runtime =
        BehaviorTreeRuntime::setup(&tree, &index(), &mut engine, false).expect("it lowers");
    runtime.tick().expect("it ticks");
    assert_eq!(get(&store, "out"), Some(Value::F64(3.0)));
}

/// A loose node that is not detached is inert: it writes nothing, so a
/// detached node writing the cell it names has that one writer.
#[test]
fn a_loose_native_writer_stays_inert() {
    use crate::nodes::{SET_STR_FUNCTION_ID, SET_STR_VALUE_PARAM_ID, SET_STR_VARIABLE_PARAM_ID};
    let store = SimpleDataStore::new();
    let loose = node(
        60,
        SET_STR_FUNCTION_ID,
        vec![
            (SET_STR_VARIABLE_PARAM_ID, key(".v")),
            (SET_STR_VALUE_PARAM_ID, key("in")),
        ],
    );
    let add = compute(50, ADD, vec![(ADD_A, f(1.0)), (ADD_B, f(1.0))], Some(".v"));
    let tree = build(&store, vec![below(1, key(".v"), 3.0)], vec![loose, add]);
    let mut engine = Engine::default();
    let mut runtime =
        BehaviorTreeRuntime::setup(&tree, &index(), &mut engine, false).expect("it lowers");
    assert_eq!(runtime.tick().expect("it ticks"), Status::Success);
}

/// An output bound to a detached node's input feeds it even when the
/// argument names a store key: the node reads its producer's result.
#[test]
fn an_output_bound_to_a_store_backed_detached_input_feeds_it() {
    let store = SimpleDataStore::new();
    set(&store, "in", 5.0);
    let mut add = compute(50, ADD, vec![(ADD_A, f(1.0)), (ADD_B, f(1.0))], None);
    add.arguments.insert(_RET_PARAM_ID, port(51, ADD_A));
    let sink = compute(
        51,
        ADD,
        vec![(ADD_A, key("in")), (ADD_B, f(1.0))],
        Some("out"),
    );
    let tree = build(&store, vec![below(1, f(1.0), 3.0)], vec![sink, add]);
    let mut engine = Engine::default();
    let mut runtime =
        BehaviorTreeRuntime::setup(&tree, &index(), &mut engine, false).expect("it lowers");
    runtime.tick().expect("it ticks");
    assert_eq!(get(&store, "out"), Some(Value::F64(3.0)));
}

/// A detached node whose call fails is called at most once in the tick: a
/// sink the tree also reads fails the tree's read, and the sink pass gets the
/// same failure without calling again. The next tick calls it again.
#[test]
fn a_failed_call_is_not_repeated_within_the_tick() {
    let store = SimpleDataStore::new();
    // `mul` errors on an absent argument.
    let failing = compute(
        50,
        MUL,
        vec![(MUL_A, key(".absent")), (MUL_B, f(1.0))],
        Some("out"),
    );
    let tree = build(&store, vec![below(1, read(50), 3.0)], vec![failing]);
    let mut engine = Engine::default();
    let calls = engine.calls.clone();
    let mut runtime =
        BehaviorTreeRuntime::setup(&tree, &index(), &mut engine, false).expect("it lowers");
    for tick in 1..=2 {
        assert!(runtime.tick().is_err(), "tick {tick} fails");
        assert_eq!(take(&calls).get("mul"), Some(&1), "tick {tick}");
    }
}

/// An output bound to a tree node's input feeds it even when the input names
/// a store key: the two share a cell, a node reading that input through its
/// port reads it too, and the key is not written.
#[test]
fn an_output_bound_to_a_store_backed_tree_input_feeds_it() {
    let store = SimpleDataStore::new();
    set(&store, "in", 5.0);
    let mut add = compute(50, ADD, vec![(ADD_A, f(1.0)), (ADD_B, f(1.0))], None);
    add.arguments.insert(_RET_PARAM_ID, port(1, BELOW_X));
    // below(x: {in} = 5, limit: 3) fails; with x = add(1, 1) it holds.
    let tree = build(
        &store,
        vec![
            control(10, SEQ_FUNCTION_ID, &[1, 2]),
            below(1, key("in"), 3.0),
            below(2, port(1, BELOW_X), 3.0),
        ],
        vec![add],
    );
    let mut engine = Engine::default();
    let mut runtime =
        BehaviorTreeRuntime::setup(&tree, &index(), &mut engine, false).expect("it lowers");
    assert_eq!(runtime.tick().expect("it ticks"), Status::Success);
    assert_eq!(
        get(&store, "in"),
        Some(Value::F64(5.0)),
        "the key is not written"
    );
    assert_eq!(tree.detached_plan().on_demand, vec![id(50)]);
}

/// An output feeds another node's input only when that input reads a cell:
/// lowering refuses an output bound to an input a call or a selection
/// computes, and a mutable parameter bound to an input naming a store key.
#[test]
fn lowering_refuses_an_output_bound_to_an_input_with_no_cell_to_feed() {
    let feeding = |target: Expression| {
        let mut add = compute(50, ADD, vec![(ADD_A, f(1.0)), (ADD_B, f(1.0))], None);
        add.arguments.insert(_RET_PARAM_ID, target);
        add
    };
    // A detached reader whose input a call computes.
    let consumer = compute(
        51,
        ADD,
        vec![
            (ADD_A, call(ADD, vec![(ADD_A, f(5.0)), (ADD_B, f(5.0))])),
            (ADD_B, f(1.0)),
        ],
        Some("out"),
    );
    let error = lowering_error(
        vec![below(1, f(1.0), 3.0)],
        vec![consumer, feeding(port(51, ADD_A))],
    );
    assert!(error.contains("a call or a selection computes"), "{error}");

    // A tree node whose input a selection computes.
    let selection = Expression::Select {
        source: Box::new(key(".x")),
        path: Key::new(".field"),
    };
    let error = lowering_error(
        vec![below(1, selection, 3.0)],
        vec![feeding(port(1, BELOW_X))],
    );
    assert!(error.contains("a call or a selection computes"), "{error}");

    // A mutable parameter bound to a tree node's input naming a store key.
    let smooth = compute(
        50,
        SMOOTH,
        vec![
            (SMOOTH_X, f(1.0)),
            (SMOOTH_ALPHA, f(1.0)),
            (SMOOTH_STATE, port(1, BELOW_X)),
        ],
        None,
    );
    let error = lowering_error(vec![below(1, key("in"), 3.0)], vec![smooth]);
    assert!(error.contains("names a store key"), "{error}");
}

/// A sink that fails stops no other: the healthy sink publishes its key, and
/// the tick reports the failure.
#[test]
fn a_failing_sink_stops_no_other() {
    let store = SimpleDataStore::new();
    // `mul` errors on an absent argument.
    let failing = compute(
        50,
        MUL,
        vec![(MUL_A, key(".absent")), (MUL_B, f(1.0))],
        Some("out"),
    );
    let healthy = compute(
        51,
        ADD,
        vec![(ADD_A, f(1.0)), (ADD_B, f(1.0))],
        Some("face/level"),
    );
    let tree = build(&store, vec![below(1, f(1.0), 3.0)], vec![failing, healthy]);
    let mut engine = Engine::default();
    let mut runtime =
        BehaviorTreeRuntime::setup(&tree, &index(), &mut engine, false).expect("it lowers");
    tree.variables.borrow()[&var(".absent")].set(Value::F64(3.0));
    runtime.tick().expect("both sinks publish");
    assert_eq!(get(&store, "out"), Some(Value::F64(3.0)));

    // The input goes absent: that sink fails, and publishes nothing, not even
    // its last result over the key's newer value.
    tree.variables.borrow()[&var(".absent")].set_opt(None);
    set(&store, "out", 9.0);
    set(&store, "face/level", 0.0);
    assert!(runtime.tick().is_err(), "the failing sink's error");
    assert_eq!(get(&store, "face/level"), Some(Value::F64(2.0)));
    assert_eq!(get(&store, "out"), Some(Value::F64(9.0)));
}
