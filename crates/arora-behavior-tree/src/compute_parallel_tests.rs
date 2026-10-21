//! Compute nodes as leaves under a root `PARALLEL`, in the order the
//! resolution finds: the scenarios of [`compute_tests`](crate::compute_tests),
//! where this form differs from on-demand, cached evaluation.
use std::collections::HashMap;

use arora_simple_data_store::SimpleDataStore;
use arora_types::data::{DataStore, Key};
use arora_types::value::Value;

use crate::call_expression_tests::{
    centimetres, control, f, index, node, Engine, ADD, ADD_A, ADD_B, BELOW_X, MUL, MUL_A, MUL_B,
    SMOOTH, SMOOTH_ALPHA, SMOOTH_STATE, SMOOTH_X,
};
use crate::compute_tests::{
    below, centimetres_nodes, compute, get, id, key, names, port, read, set, take, three_readers,
    var,
};
use crate::display::Names;
use crate::nodes::{FALLBACK_FUNCTION_ID, SEQ_FUNCTION_ID};
use crate::schema::{Node, _RET_PARAM_ID};
use crate::{
    edge_variable, load_behavior_tree_with_compute, load_behavior_tree_with_compute_as_leaves,
    BehaviorTree, BehaviorTreeRuntime, ComputeNode,
};

fn build(store: &SimpleDataStore, nodes: Vec<Node>, compute: Vec<ComputeNode>) -> BehaviorTree {
    let resolver = |name: &str| Some(store.slot(&Key::from(name)));
    load_behavior_tree_with_compute_as_leaves(nodes, compute, &resolver, &names())
        .expect("the tree builds")
}

/// The leaves follow the data, whatever order the nodes are given in, and
/// the tree comes after them: it reads this tick's values.
#[test]
fn the_resolution_orders_the_leaves_ahead_of_the_tree() {
    let store = SimpleDataStore::new();
    set(&store, "sensor/distance", 0.2);
    let mut nodes = centimetres_nodes();
    nodes.push(compute(
        12,
        ADD,
        vec![(ADD_A, read(11)), (ADD_B, f(1.0))],
        Some("face/level"),
    ));
    nodes.reverse();
    let tree = build(&store, vec![below(1, read(11), 30.0)], nodes);

    let functions = index();
    let mut variables = names();
    variables.insert(edge_variable(id(10)), ".#raw".to_string());
    variables.insert(edge_variable(id(11)), ".#cm".to_string());
    let names = Names {
        functions: Some(&functions),
        variables: Some(&variables),
        ..Default::default()
    };
    assert_eq!(
        tree.display(names),
        "PARALLEL\n\
         \x20 {.#raw} = mul(a: {sensor/distance}, b: 100f64)\n\
         \x20 {.#cm} = clamp(x: {.#raw}, lo: 0f64)\n\
         \x20 {face/level} = add(a: {.#cm}, b: 1f64)\n\
         \x20 below(x: {.#cm}, limit: 30f64)\n"
    );

    let mut engine = Engine::default();
    let calls = engine.calls.clone();
    let mut runtime = BehaviorTreeRuntime::setup(&tree, &index(), &mut engine, false).unwrap();
    runtime.tick().unwrap();
    assert_eq!(get(&store, "face/level"), Some(Value::F64(21.0)));
    let below_x = calls
        .borrow()
        .iter()
        .find(|(name, _)| *name == "below")
        .map(|(_, args)| args[&BELOW_X].clone());
    assert_eq!(
        below_x,
        Some(Value::F64(20.0)),
        "the tree reads this tick's value"
    );
}

/// A node read by three leaves is one leaf: it runs once per tick. It also
/// runs at every tick, settled or not: nothing is cached.
#[test]
fn a_shared_node_runs_once_per_tick_and_every_tick() {
    let store = SimpleDataStore::new();
    set(&store, "sensor/distance", 0.2);
    let tree = build(&store, three_readers(|| read(11)), centimetres_nodes());
    let mut engine = Engine::default();
    let calls = engine.calls.clone();
    let mut runtime = BehaviorTreeRuntime::setup(&tree, &index(), &mut engine, false).unwrap();
    for _ in 0..2 {
        runtime.tick().unwrap();
        let counts = take(&calls);
        assert_eq!((counts["mul"], counts["clamp"], counts["below"]), (1, 1, 3));
    }
}

/// The node an untaken fallback child reads still runs, every tick: the
/// leaves run whatever the tree does. A node nothing reads is not a leaf.
#[test]
fn a_node_runs_whether_or_not_its_reader_ticks() {
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
    let calls = engine.calls.clone();
    let mut runtime = BehaviorTreeRuntime::setup(&tree, &index(), &mut engine, false).unwrap();
    runtime.tick().unwrap();
    assert_eq!(
        take(&calls),
        HashMap::from([("mul", 2), ("below", 1)]),
        "#21 runs for the child that does not tick; #29 is not a leaf"
    );
}

/// A rogue node writing a store key is a leaf like any other: it runs and
/// writes its key every tick. A rogue node writing a tree-local variable is
/// not a leaf.
#[test]
fn a_rogue_node_writing_a_store_key_runs_every_tick() {
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
    let calls = engine.calls.clone();
    let mut runtime = BehaviorTreeRuntime::setup(&tree, &index(), &mut engine, false).unwrap();
    runtime.tick().unwrap();
    set(&store, "out", -1.0);
    runtime.tick().unwrap();
    assert_eq!(get(&store, "out"), Some(Value::F64(6.0)));
    assert_eq!(take(&calls)["mul"], 2, "two ticks, one call each");
}

/// Of two nodes writing one key, the later leaf wins.
#[test]
fn the_later_of_two_nodes_writing_one_key_wins() {
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
    set(&store, "x", 7.0);
    runtime.tick().unwrap();
    assert_eq!(get(&store, "out"), Some(Value::F64(2.0)));
}

/// A stateful node keeps its state in its leaf's argument cell, which the
/// tree reads as `#node.state`. It runs every tick, settled or not.
#[test]
fn a_stateful_node_runs_every_tick() {
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
    let mut state_read = Vec::new();
    for _ in 0..4 {
        runtime.tick().unwrap();
        let ticked = calls.borrow_mut().drain(..).collect::<Vec<_>>();
        smooth_calls.push(ticked.iter().filter(|(name, _)| *name == "smooth").count());
        state_read.extend(
            ticked
                .iter()
                .filter(|(name, _)| *name == "below")
                .map(|(_, args)| args[&BELOW_X].clone()),
        );
    }
    assert_eq!(smooth_calls, [1, 1, 1, 1]);
    assert_eq!(state_read, vec![Value::F64(2.0); 4]);
    assert_eq!(get(&store, "out"), Some(Value::F64(2.0)));
}

/// The leaves run before the tree: a variable the tree writes reaches them at
/// the next tick, and both reads of a tick see the same value.
#[test]
fn the_tree_s_writes_reach_the_leaves_at_the_next_tick() {
    let store = SimpleDataStore::new();
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
    let readings = || -> Vec<Value> {
        calls
            .borrow_mut()
            .drain(..)
            .filter(|(name, _)| *name == "below")
            .map(|(_, args)| args[&BELOW_X].clone())
            .collect()
    };
    runtime.tick().unwrap();
    assert_eq!(readings(), [Value::F64(0.0), Value::F64(0.0)]);
    runtime.tick().unwrap();
    assert_eq!(readings(), [Value::F64(20.0), Value::F64(20.0)]);
}

/// Building refuses what the resolution refuses, and a compute node with a
/// tree node's id.
#[test]
fn building_refuses_a_cycle_and_a_shared_id() {
    let store = SimpleDataStore::new();
    let resolver = |name: &str| Some(store.slot(&Key::from(name)));
    let error = |nodes, compute| match load_behavior_tree_with_compute_as_leaves(
        nodes,
        compute,
        &resolver,
        &names(),
    ) {
        Ok(_) => panic!("the tree builds"),
        Err(error) => error.to_string(),
    };
    let message = error(
        vec![below(1, read(80), 1.0)],
        vec![
            compute(80, ADD, vec![(ADD_A, read(81)), (ADD_B, f(1.0))], None),
            compute(81, ADD, vec![(ADD_A, read(80)), (ADD_B, f(1.0))], None),
        ],
    );
    assert!(message.contains("in a cycle"), "{message}");
    let message = error(
        vec![below(1, f(1.0), 1.0)],
        vec![compute(
            1,
            ADD,
            vec![(ADD_A, f(1.0)), (ADD_B, f(1.0))],
            Some("out"),
        )],
    );
    assert!(message.contains("has the id of a tree node"), "{message}");
}

/// The three forms over a hundred ticks: the cascade read by three leaves,
/// the distance changing every tenth tick.
#[test]
fn over_a_hundred_ticks_the_leaves_run_once_per_tick() {
    let store = SimpleDataStore::new();
    let resolver = |name: &str| Some(store.slot(&Key::from(name)));
    let forms: [(&str, BehaviorTree); 3] = [
        (
            "call expressions",
            load_behavior_tree_with_compute(
                three_readers(centimetres),
                Vec::new(),
                &resolver,
                &names(),
            )
            .unwrap(),
        ),
        (
            "compute nodes",
            load_behavior_tree_with_compute(
                three_readers(|| read(11)),
                centimetres_nodes(),
                &resolver,
                &names(),
            )
            .unwrap(),
        ),
        (
            "leaves",
            build(&store, three_readers(|| read(11)), centimetres_nodes()),
        ),
    ];
    let mut rows = Vec::new();
    for (form, tree) in &forms {
        let mut engine = Engine::default();
        let sensors = engine.sensors.clone();
        let calls = engine.calls.clone();
        let mut runtime = BehaviorTreeRuntime::setup(tree, &index(), &mut engine, false).unwrap();
        let mut total: HashMap<&'static str, usize> = HashMap::new();
        for tick in 0..100 {
            let distance = 0.1 * (tick / 10) as f64;
            set(&store, "sensor/distance", distance);
            sensors
                .borrow_mut()
                .insert(crate::call_expression_tests::DISTANCE, distance);
            runtime.tick().unwrap();
            for (name, count) in take(&calls) {
                *total.entry(name).or_insert(0) += count;
            }
        }
        let row =
            ["distance", "mul", "clamp", "below"].map(|name| total.get(name).copied().unwrap_or(0));
        eprintln!("{form:>16}: {row:?}");
        rows.push(row);
    }
    assert_eq!(
        rows,
        [[300, 300, 300, 300], [0, 10, 10, 300], [0, 100, 100, 300]]
    );
}
