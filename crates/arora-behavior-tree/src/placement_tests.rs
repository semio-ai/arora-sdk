//! Placement contracts: explicit structures, equivalent pure graphs, and
//! the tick-order differences that prevent general detached equivalence.
use crate::incremental_tests::fixture;

use crate::nodes::*;
use crate::schema::{
    CallExpression, Expression, Node, NodeParameterId, _ON_CHANGE_PARAM_ID, _RET_PARAM_ID,
};
use crate::{
    load_behavior_tree_nodes_with, lower_behavior_tree, place_detached, LoweredTree, PlacementScope,
};
use arora_behavior::Status;
use arora_simple_data_store::SimpleDataStore;
use arora_types::data::{DataStore, Key};
use arora_types::value::Value;
use fixture::{Bridge, Op, MODULE, OUT, STATE, X, Y};
use std::collections::HashMap;
use uuid::Uuid;

fn id(n: u128) -> Uuid {
    Uuid::from_u128(n)
}
fn lit(n: f64) -> Expression {
    Expression::Value(Value::F64(n))
}
fn var(n: u128) -> Expression {
    Expression::VariableId(id(n))
}
fn port(n: u128) -> Expression {
    parameter(n, _RET_PARAM_ID)
}
fn parameter(n: u128, parameter: Uuid) -> Expression {
    Expression::NodeArgument(NodeParameterId {
        node: id(n),
        parameter,
    })
}
fn node(n: u128, function: Uuid, arguments: Vec<(Uuid, Expression)>) -> Node {
    Node {
        id: id(n),
        function,
        arguments: arguments.into_iter().collect(),
        children: None,
    }
}
fn control(n: u128, function: Uuid, children: &[u128]) -> Node {
    Node {
        children: Some(children.iter().copied().map(id).collect()),
        ..node(n, function, vec![])
    }
}
fn module(n: u128, input: Expression) -> Node {
    node(
        n,
        id(20000 + n),
        vec![(X, input), (Y, lit(2.0)), (_RET_PARAM_ID, var(30000 + n))],
    )
}
fn cached(mut node: Node) -> Node {
    node.arguments
        .insert(_ON_CHANGE_PARAM_ID, Expression::Value(Value::Boolean(true)));
    node
}
fn names() -> HashMap<Uuid, String> {
    [
        (9000, "input/a"),
        (9001, "input/b"),
        (9002, "input/c"),
        (9003, "gate"),
        (9004, "out/a"),
        (9005, "out/b"),
        (9006, "out/a"),
    ]
    .into_iter()
    .map(|(n, s)| (id(n), s.into()))
    .collect()
}
fn bridge(nodes: &[Node]) -> Bridge {
    let mut bridge = Bridge::default();
    for node in nodes {
        if node.function.as_u128() >= 20000 && node.function.as_u128() < 30000 {
            bridge.ops.insert(node.function, Op::Add);
        }
    }
    bridge
}
fn place(
    nodes: Vec<Node>,
    scope: PlacementScope,
    bridge: &Bridge,
    store: &SimpleDataStore,
) -> Vec<Node> {
    place_detached(
        nodes,
        scope,
        &bridge.index(),
        &|name| Some(store.slot(&Key::from(name))),
        &names(),
    )
    .unwrap()
}
fn lower(
    nodes: Vec<Node>,
    bridge: &mut Bridge,
    store: &SimpleDataStore,
    incremental: bool,
) -> LoweredTree {
    let tree =
        load_behavior_tree_nodes_with(nodes, &|name| Some(store.slot(&Key::from(name))), &names())
            .unwrap();
    let tree = lower_behavior_tree(tree, &bridge.index(), bridge, false).unwrap();
    tree.set_incremental(incremental);
    tree
}
fn put(store: &SimpleDataStore, name: &str, x: f64) {
    store
        .slot(&Key::from(name))
        .set(Some(Value::F64(x)))
        .unwrap();
}
fn value(tree: &LoweredTree, n: u128, parameter: Uuid) -> Option<Value> {
    tree.tree().node_arg_variables[&NodeParameterId {
        node: id(n),
        parameter,
    }]
        .get()
}
fn tick(tree: &LoweredTree, bridge: &mut Bridge) -> Status {
    bridge.reset_counts();
    tree.tick(bridge).unwrap()
}
const W1: u128 = 0x57544800000040008000000000000001;
const W2: u128 = 0x57544800000040008000000000000002;
const W3: u128 = 0x57544800000040008000000000000003;
const G1_10: u128 = 0x475250000000400a8000000000000001;
const G2_10: u128 = 0x475250000000400a8000000000000002;
const G3_11: u128 = 0x475250000000400b8000000000000003;

/// Expected lists spell out the anchor, wrappers, group order and leaf order;
/// they do not derive an expected tree using placement's algorithm.
#[test]
fn placement_structures_are_explicit_and_deterministic() {
    let single = || {
        vec![
            control(1, SEQ_FUNCTION_ID, &[2]),
            module(2, port(10)),
            module(10, var(9000)),
        ]
    };
    let nested = || {
        vec![
            control(1, FALLBACK_FUNCTION_ID, &[2, 5]),
            control(2, SEQ_FUNCTION_ID, &[3, 4]),
            module(3, port(10)),
            module(4, port(10)),
            node(5, FAIL_FUNCTION_ID, vec![]),
            module(10, var(9000)),
        ]
    };
    let two = || {
        vec![
            control(1, SEQ_FUNCTION_ID, &[2, 3]),
            module(2, port(10)),
            module(3, port(11)),
            module(10, var(9000)),
            module(11, var(9001)),
        ]
    };
    let chain = || {
        vec![
            control(1, SEQ_FUNCTION_ID, &[2]),
            module(2, port(11)),
            module(11, port(10)),
            module(10, var(9000)),
        ]
    };
    let sink = || {
        let mut nodes = single();
        nodes[2].arguments.insert(_RET_PARAM_ID, var(9004));
        nodes
    };
    let shared = || {
        vec![
            control(1, FALLBACK_FUNCTION_ID, &[3, 4]),
            control(3, SEQ_FUNCTION_ID, &[2]),
            control(4, SEQ_FUNCTION_ID, &[2]),
            module(2, port(10)),
            module(10, var(9000)),
        ]
    };
    let cases = vec![
        (
            single(),
            vec![
                control(1, SEQ_FUNCTION_ID, &[W2]),
                module(2, port(10)),
                cached(module(10, var(9000))),
                control(W2, WITH_FUNCTION_ID, &[G2_10, 2]),
                control(G2_10, PARALLEL_FUNCTION_ID, &[10]),
            ],
        ),
        (
            nested(),
            vec![
                control(1, FALLBACK_FUNCTION_ID, &[W2, 5]),
                control(2, SEQ_FUNCTION_ID, &[3, 4]),
                module(3, port(10)),
                module(4, port(10)),
                node(5, FAIL_FUNCTION_ID, vec![]),
                cached(module(10, var(9000))),
                control(W2, WITH_FUNCTION_ID, &[G2_10, 2]),
                control(G2_10, PARALLEL_FUNCTION_ID, &[10]),
            ],
        ),
        (
            two(),
            vec![
                control(1, SEQ_FUNCTION_ID, &[W2, W3]),
                module(2, port(10)),
                module(3, port(11)),
                cached(module(10, var(9000))),
                cached(module(11, var(9001))),
                control(W2, WITH_FUNCTION_ID, &[G2_10, 2]),
                control(G2_10, PARALLEL_FUNCTION_ID, &[10]),
                control(W3, WITH_FUNCTION_ID, &[G3_11, 3]),
                control(G3_11, PARALLEL_FUNCTION_ID, &[11]),
            ],
        ),
        (
            chain(),
            vec![
                control(1, SEQ_FUNCTION_ID, &[W2]),
                module(2, port(11)),
                cached(module(11, port(10))),
                cached(module(10, var(9000))),
                control(
                    W2,
                    WITH_FUNCTION_ID,
                    &[0x475250000000400b8000000000000002, 2],
                ),
                control(
                    0x475250000000400b8000000000000002,
                    PARALLEL_FUNCTION_ID,
                    &[10, 11],
                ),
            ],
        ),
        (
            sink(),
            vec![
                control(
                    W1,
                    WITH_FUNCTION_ID,
                    &[G1_10, 0x50554247000040008000000000000001, 1],
                ),
                control(1, SEQ_FUNCTION_ID, &[2]),
                module(2, port(10)),
                cached(node(
                    10,
                    id(20010),
                    vec![
                        (X, var(9000)),
                        (Y, lit(2.0)),
                        (_RET_PARAM_ID, var(0xd04f520000004000df7265740000400a)),
                    ],
                )),
                node(
                    0xd055420000004000df7265740000400a,
                    COPY_FUNCTION_ID,
                    vec![
                        (COPY_VALUE_PARAM_ID, port(10)),
                        (COPY_OUT_PARAM_ID, var(9004)),
                    ],
                ),
                control(G1_10, PARALLEL_FUNCTION_ID, &[10]),
                control(
                    0x50554247000040008000000000000001,
                    PARALLEL_FUNCTION_ID,
                    &[0xd055420000004000df7265740000400a],
                ),
            ],
        ),
        (
            shared(),
            vec![
                control(W1, WITH_FUNCTION_ID, &[G1_10, 1]),
                control(1, FALLBACK_FUNCTION_ID, &[3, 4]),
                control(3, SEQ_FUNCTION_ID, &[2]),
                control(4, SEQ_FUNCTION_ID, &[2]),
                module(2, port(10)),
                cached(module(10, var(9000))),
                control(G1_10, PARALLEL_FUNCTION_ID, &[10]),
            ],
        ),
        (
            vec![
                module(1, port(10)),
                module(10, var(9000)),
                module(12, lit(9.0)),
            ],
            vec![
                control(W1, WITH_FUNCTION_ID, &[G1_10, 1]),
                module(1, port(10)),
                cached(module(10, var(9000))),
                module(12, lit(9.0)),
                control(G1_10, PARALLEL_FUNCTION_ID, &[10]),
            ],
        ),
    ];
    for (input, expected) in cases {
        let bridge = bridge(&input);
        for _ in 0..2 {
            assert_eq!(
                place(
                    input.clone(),
                    PlacementScope::Readers,
                    &bridge,
                    &SimpleDataStore::new()
                ),
                expected
            );
        }
    }
    let input = two();
    let bridge = bridge(&input);
    assert_eq!(
        place(
            input,
            PlacementScope::Root,
            &bridge,
            &SimpleDataStore::new()
        ),
        vec![
            control(
                W1,
                WITH_FUNCTION_ID,
                &[G1_10, 0x475250000000400b8000000000000001, 1]
            ),
            control(1, SEQ_FUNCTION_ID, &[2, 3]),
            module(2, port(10)),
            module(3, port(11)),
            cached(module(10, var(9000))),
            cached(module(11, var(9001))),
            control(G1_10, PARALLEL_FUNCTION_ID, &[10]),
            control(
                0x475250000000400b8000000000000001,
                PARALLEL_FUNCTION_ID,
                &[11]
            )
        ]
    );
}

/// Ignored statuses include Failure and Running; errors still stop the body.
#[test]
fn with_returns_its_body_and_propagates_errors() {
    for incremental in [false, true] {
        let nodes = vec![
            control(1, WITH_FUNCTION_ID, &[2, 3, 4]),
            node(2, FAIL_FUNCTION_ID, vec![]),
            node(3, RUN_FUNCTION_ID, vec![]),
            node(4, SUCCEED_FUNCTION_ID, vec![]),
        ];
        let mut bridge = Bridge::default();
        let tree = lower(nodes, &mut bridge, &SimpleDataStore::new(), incremental);
        assert_eq!(tick(&tree, &mut bridge), Status::Success);
        assert_eq!(tick(&tree, &mut bridge), Status::Success);
        let nodes = vec![
            control(1, WITH_FUNCTION_ID, &[2, 3]),
            control(2, WITH_FUNCTION_ID, &[]),
            module(3, lit(0.0)),
        ];
        let mut bridge = bridge_for(&nodes);
        let tree = lower(nodes, &mut bridge, &SimpleDataStore::new(), incremental);
        assert!(tree
            .tick(&mut bridge)
            .unwrap_err()
            .to_string()
            .contains("at least two"));
        assert!(bridge.calls.is_empty());
    }
    assert!(control(1, WITH_FUNCTION_ID, &[2, 3])
        .display(crate::display::Names::default())
        .to_string()
        .starts_with("WITH("));
}
fn bridge_for(nodes: &[Node]) -> Bridge {
    bridge(nodes)
}

/// A hand-authored WITH tree and automatic placement have identical calls,
/// cells, statuses and stores over 320 input steps, including settled periods.
#[test]
fn placement_and_a_hand_written_with_tree_run_in_lockstep() {
    let source = vec![
        control(1, SEQ_FUNCTION_ID, &[2]),
        module(2, parameter(11, STATE)),
        node(
            11,
            id(20011),
            vec![
                (X, port(10)),
                (Y, lit(3.0)),
                (STATE, lit(0.0)),
                (_RET_PARAM_ID, var(9004)),
            ],
        ),
        module(10, var(9000)),
    ];
    let mut hand_sink = source[2].clone();
    hand_sink.arguments.insert(_RET_PARAM_ID, var(31011));
    let hand = vec![
        control(100, WITH_FUNCTION_ID, &[101, 1]),
        control(101, PARALLEL_FUNCTION_ID, &[10, 11, 102]),
        control(1, SEQ_FUNCTION_ID, &[2]),
        source[1].clone(),
        cached(hand_sink),
        node(
            102,
            COPY_FUNCTION_ID,
            vec![
                (COPY_VALUE_PARAM_ID, port(11)),
                (COPY_OUT_PARAM_ID, var(9004)),
            ],
        ),
        cached(source[3].clone()),
    ];
    let stores = [SimpleDataStore::new(), SimpleDataStore::new()];
    let mut a = bridge(&source);
    a.ops.insert(id(20011), Op::Effect);
    let mut b = bridge(&source);
    b.ops.insert(id(20011), Op::Effect);
    let placed = place(source.clone(), PlacementScope::Readers, &a, &stores[0]);
    let trees = [
        lower(placed, &mut a, &stores[0], true),
        lower(hand, &mut b, &stores[1], true),
    ];
    for step in 0..320 {
        for store in &stores {
            put(store, "input/a", ((step / 5) % 17) as f64);
        }
        assert_eq!(tick(&trees[0], &mut a), tick(&trees[1], &mut b));
        assert_eq!(a.calls, b.calls);
        assert_eq!(stores[0].snapshot(), stores[1].snapshot());
        for node in &source {
            for parameter in node.arguments.keys() {
                assert_eq!(
                    value(&trees[0], node.id.as_u128(), *parameter),
                    value(&trees[1], node.id.as_u128(), *parameter)
                );
            }
        }
    }
}

struct Rng(u64);
impl Rng {
    fn next(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 as usize) % n
    }
}

/// Pure detached functions read only store inputs set between ticks and other
/// detached ports. No attached node writes a cell a detached node reads; no
/// attached node reads a key a sink publishes; detached functions keep no state.
/// These restrictions make hoisting equivalent despite different call counts.
/// 240 seeded trees × 240 ticks × both scopes compare full store snapshots,
/// root statuses and ordered calls, including arguments, to all attached functions.
#[test]
fn pure_generated_detached_and_placed_trees_agree() {
    let mut rng = Rng(0x709c45c2c1968081);
    for case in 0..240 {
        let count = 3 + rng.next(8);
        let mut nodes = vec![control(
            1,
            [SEQ_FUNCTION_ID, FALLBACK_FUNCTION_ID, PARALLEL_FUNCTION_ID][rng.next(3)],
            &[2, 3, 4],
        )];
        for branch in 2..=4 {
            nodes.push(control(
                branch,
                [SEQ_FUNCTION_ID, FALLBACK_FUNCTION_ID, PARALLEL_FUNCTION_ID][rng.next(3)],
                &[branch * 10, branch * 10 + 1],
            ));
            nodes.push(node(
                branch * 10,
                EQUAL_FUNCTION_ID,
                vec![
                    (EQUAL_A_PARAM_ID, var(9003)),
                    (EQUAL_B_PARAM_ID, lit(rng.next(3) as f64)),
                ],
            ));
            let mut leaf = module(branch * 10 + 1, port(100 + rng.next(count) as u128));
            leaf.arguments.insert(STATE, lit(0.0));
            nodes.push(leaf);
        }
        // Occasionally share a leaf across branches, so all reader paths matter.
        if case % 3 == 0 {
            nodes[4].children.as_mut().unwrap().push(id(21));
        }
        for i in 0..count {
            let input = if i == 0 || rng.next(3) == 0 {
                var(9000 + rng.next(3) as u128)
            } else {
                port(100 + rng.next(i) as u128)
            };
            let mut leaf = module(100 + i as u128, input);
            if rng.next(3) == 0 {
                leaf.arguments.insert(
                    Y,
                    if i == 0 {
                        var(9001)
                    } else {
                        port(100 + rng.next(i) as u128)
                    },
                );
            }
            if i == count - 1 {
                leaf.arguments.insert(_RET_PARAM_ID, var(9004));
            }
            nodes.push(leaf);
        }
        // The file order need not be its dependency order.
        for i in 0..count {
            let at = nodes.len() - 1 - i;
            let other = nodes.len() - 1 - rng.next(count);
            nodes.swap(at, other);
        }
        let stores = [
            SimpleDataStore::new(),
            SimpleDataStore::new(),
            SimpleDataStore::new(),
        ];
        let mut bridges: Vec<_> = (0..3)
            .map(|_| {
                let mut b = bridge(&nodes);
                for i in 0..count {
                    b.ops.insert(
                        id(20100 + i as u128),
                        [Op::Add, Op::Mul, Op::Clamp, Op::Remap][(case + i) % 4],
                    );
                }
                for leaf in [21, 31, 41] {
                    b.ops.insert(id(20000 + leaf), Op::Effect);
                }
                b
            })
            .collect();
        let mut trees = Vec::new();
        for (mode, scope) in [
            None,
            Some(PlacementScope::Readers),
            Some(PlacementScope::Root),
        ]
        .into_iter()
        .enumerate()
        {
            let file = scope.map_or_else(
                || nodes.clone(),
                |scope| place(nodes.clone(), scope, &bridges[mode], &stores[mode]),
            );
            if scope.is_some() {
                assert!(
                    file.iter().any(|node| node.function == WITH_FUNCTION_ID),
                    "case {case}"
                );
                let original = load_behavior_tree_nodes_with(
                    nodes.clone(),
                    &|name| Some(stores[mode].slot(&Key::from(name))),
                    &names(),
                )
                .unwrap();
                original.lower_detached(&bridges[mode].index()).unwrap();
                let plan = original.detached_plan();
                let attached = attached_ids(&file);
                for used in plan.every_tick.iter().chain(&plan.on_demand) {
                    assert!(
                        attached.contains(used),
                        "used node {used} stayed detached in case {case}"
                    );
                }
            }
            trees.push(lower(file, &mut bridges[mode], &stores[mode], mode != 0));
        }
        let mut inputs = [0.0; 4];
        for step in 0..240 {
            if step % 4 == 0 {
                let key = rng.next(4);
                inputs[key] = rng.next(11) as f64;
            }
            for store in &stores {
                for (key, x) in ["input/a", "input/b", "input/c", "gate"]
                    .into_iter()
                    .zip(inputs)
                {
                    put(store, key, x);
                }
            }
            let mut results = Vec::new();
            let mut calls = Vec::new();
            for mode in 0..3 {
                results.push(tick(&trees[mode], &mut bridges[mode]));
                calls.push(
                    bridges[mode]
                        .calls
                        .iter()
                        .filter(|(function, _)| function.as_u128() < 20100)
                        .cloned()
                        .collect::<Vec<_>>(),
                );
            }
            for mode in 1..3 {
                assert_eq!(
                    results[0], results[mode],
                    "case {case}, tick {step}, mode {mode}"
                );
                assert_eq!(
                    calls[0], calls[mode],
                    "case {case}, tick {step}, mode {mode}"
                );
                assert_eq!(
                    stores[0].snapshot(),
                    stores[mode].snapshot(),
                    "case {case}, tick {step}, mode {mode}"
                );
            }
        }
    }
}

/// Root placement advances state even when the reader's branch is not taken;
/// reader placement keeps that state dormant until its anchor is reached.
#[test]
fn root_placement_runs_state_outside_an_unreached_reader_branch() {
    let source = vec![
        control(1, SEQ_FUNCTION_ID, &[2, 3]),
        node(2, FAIL_FUNCTION_ID, vec![]),
        module(3, parameter(10, STATE)),
        node(10, id(20010), vec![(STATE, lit(0.0))]),
    ];
    for (scope, expected) in [
        (None, 0),
        (Some(PlacementScope::Root), 8),
        (Some(PlacementScope::Readers), 0),
    ] {
        let store = SimpleDataStore::new();
        let mut bridge = bridge(&source);
        bridge.ops.insert(id(20010), Op::Effect);
        let file = scope.map_or_else(
            || source.clone(),
            |scope| place(source.clone(), scope, &bridge, &store),
        );
        let tree = lower(file, &mut bridge, &store, scope.is_some());
        for _ in 0..8 {
            assert_eq!(tick(&tree, &mut bridge), Status::Failure);
        }
        assert_eq!(value(&tree, 10, STATE), Some(Value::F64(expected as f64)));
    }
}

/// The common anchor precedes an attached writer; a detached pull at the
/// reader instead observes the write immediately. Hoisting delays it one tick.
#[test]
fn placement_before_an_attached_writer_reads_its_value_next_tick() {
    let mut writer = module(2, var(9000));
    writer.arguments.insert(_RET_PARAM_ID, var(9000));
    writer.arguments.insert(Y, lit(1.0));
    let source = vec![
        control(1, SEQ_FUNCTION_ID, &[2, 3, 4]),
        writer,
        module(3, port(10)),
        module(4, port(10)),
        module(10, var(9000)),
    ];
    for scope in [
        None,
        Some(PlacementScope::Readers),
        Some(PlacementScope::Root),
    ] {
        let store = SimpleDataStore::new();
        put(&store, "input/a", 0.0);
        let mut bridge = bridge(&source);
        let file = scope.map_or_else(
            || source.clone(),
            |scope| place(source.clone(), scope, &bridge, &store),
        );
        let tree = lower(file, &mut bridge, &store, scope.is_some());
        for step in 0..4 {
            tick(&tree, &mut bridge);
            let expected = step as f64 + if scope.is_some() { 2.0 } else { 3.0 };
            assert_eq!(value(&tree, 10, _RET_PARAM_ID), Some(Value::F64(expected)));
            for reader in [3, 4] {
                assert_eq!(value(&tree, reader, X), Some(Value::F64(expected)));
            }
        }
    }
}

/// A sink is published after a detached tick and before the placed body.
#[test]
fn placement_publishes_a_sink_key_in_the_same_tick() {
    let mut sink = module(10, var(9000));
    sink.arguments.insert(_RET_PARAM_ID, var(9004));
    let source = vec![module(1, var(9004)), sink];
    for scope in [
        None,
        Some(PlacementScope::Readers),
        Some(PlacementScope::Root),
    ] {
        let store = SimpleDataStore::new();
        put(&store, "out/a", 0.0);
        let mut bridge = bridge(&source);
        let file = scope.map_or_else(
            || source.clone(),
            |scope| place(source.clone(), scope, &bridge, &store),
        );
        let tree = lower(file, &mut bridge, &store, scope.is_some());
        for step in 0..4 {
            put(&store, "input/a", step as f64);
            tick(&tree, &mut bridge);
            let expected = if scope.is_some() {
                step as f64 + 2.0
            } else if step == 0 {
                0.0
            } else {
                step as f64 + 1.0
            };
            let x = bridge
                .calls
                .iter()
                .find(|(id, _)| *id == Uuid::from_u128(20001))
                .unwrap()
                .1
                .iter()
                .find(|field| field.id == X)
                .unwrap()
                .value
                .as_ref();
            assert_eq!(x, &Value::F64(expected));
        }
    }
}

/// Attachment preserves implicit returns, local-variable reads, and a return
/// feeding a store-named input without publishing to that store key.
#[test]
fn placement_preserves_implicit_ports_and_fed_input_isolation() {
    for binding in ["implicit", "local", "fed store input"] {
        let mut source = vec![module(1, port(10)), module(10, var(9000))];
        match binding {
            "implicit" => {
                source[1].arguments.remove(&_RET_PARAM_ID);
            }
            "local" => {
                source[0].arguments.insert(X, var(30010));
            }
            _ => {
                source[0].arguments.insert(X, var(9001));
                source[1].arguments.insert(_RET_PARAM_ID, parameter(1, X));
            }
        }
        let stores = [SimpleDataStore::new(), SimpleDataStore::new()];
        let mut a = bridge(&source);
        let mut b = bridge(&source);
        let file = place(source.clone(), PlacementScope::Readers, &b, &stores[1]);
        let trees = [
            lower(source, &mut a, &stores[0], false),
            lower(file, &mut b, &stores[1], true),
        ];
        for step in 0..8 {
            for store in &stores {
                put(store, "input/a", step as f64);
                put(store, "input/b", 99.0);
            }
            assert_eq!(tick(&trees[0], &mut a), tick(&trees[1], &mut b));
            assert_eq!(value(&trees[0], 1, X), Some(Value::F64(step as f64 + 2.0)));
            assert_eq!(value(&trees[0], 1, X), value(&trees[1], 1, X));
            assert_eq!(stores[0].snapshot(), stores[1].snapshot());
            assert_eq!(a.calls, b.calls);
        }
    }
}

/// A settled WITH and all its children can be skipped, and an independent
/// settled group remains cached while the body's RUN is polled.
#[test]
fn with_caches_prerequisites_independently_of_a_running_body() {
    for running in [false, true] {
        let source = vec![
            control(1, WITH_FUNCTION_ID, &[2, 3]),
            control(2, PARALLEL_FUNCTION_ID, &[10]),
            node(
                3,
                if running {
                    RUN_FUNCTION_ID
                } else {
                    SUCCEED_FUNCTION_ID
                },
                vec![],
            ),
            cached(module(10, var(9000))),
        ];
        let store = SimpleDataStore::new();
        let mut bridge = bridge(&source);
        let tree = lower(source, &mut bridge, &store, true);
        tick(&tree, &mut bridge);
        tick(&tree, &mut bridge);
        assert!(bridge.calls.is_empty());
        assert_eq!(bridge.dispatches, if running { 2 } else { 0 });
        put(&store, "input/a", 3.0);
        tick(&tree, &mut bridge);
        assert_eq!(bridge.calls.len(), 1);
        tick(&tree, &mut bridge);
        assert!(bridge.calls.is_empty());
        assert_eq!(bridge.dispatches, if running { 2 } else { 0 });
    }
}

/// Invalid detached cycles and ambiguous writers use the runtime's errors;
/// malformed attached paths fail finitely, and generated-id collisions probe
/// deterministically without replacing an existing file node.
#[test]
fn placement_reuses_validation_and_handles_id_collisions() {
    for source in [
        vec![
            module(1, port(10)),
            module(10, port(11)),
            module(11, port(10)),
        ],
        vec![
            module(1, port(10)),
            module(10, var(9000)),
            node(11, id(20011), vec![(_RET_PARAM_ID, var(30010))]),
        ],
        vec![control(1, SEQ_FUNCTION_ID, &[1])],
        vec![control(1, SEQ_FUNCTION_ID, &[2])],
    ] {
        let store = SimpleDataStore::new();
        let bridge = bridge(&source);
        let error = place_detached(
            source,
            PlacementScope::Readers,
            &bridge.index(),
            &|name| Some(store.slot(&Key::from(name))),
            &names(),
        );
        assert!(error.is_err());
    }
    let source = vec![
        control(1, SEQ_FUNCTION_ID, &[2]),
        module(2, port(10)),
        module(10, var(9000)),
        node(W2, SUCCEED_FUNCTION_ID, vec![]),
        module(11, port(10)),
    ];
    let bridge = bridge(&source);
    let store = SimpleDataStore::new();
    let placed = place(source.clone(), PlacementScope::Readers, &bridge, &store);
    assert_eq!(
        placed,
        place(source, PlacementScope::Readers, &bridge, &store)
    );
    assert_eq!(
        placed
            .iter()
            .map(|node| node.id)
            .collect::<std::collections::HashSet<_>>()
            .len(),
        placed.len()
    );
    assert!(placed
        .iter()
        .any(|node| node.id == id(W2) && node.function == SUCCEED_FUNCTION_ID));
    assert!(
        placed.iter().any(|node| node.id == id(11)),
        "unused downstream node is retained"
    );
}

/// An unread node neither merges live groups nor lifts their reader anchors.
#[test]
fn unread_nodes_do_not_connect_components_or_lift_anchors() {
    let source = vec![
        control(1, SEQ_FUNCTION_ID, &[2, 3]),
        module(2, port(10)),
        module(3, port(11)),
        node(12, id(20012), vec![(X, port(10)), (Y, port(11))]),
        module(10, var(9000)),
        module(11, var(9001)),
    ];
    let bridge = bridge(&source);
    assert_eq!(
        place(
            source.clone(),
            PlacementScope::Readers,
            &bridge,
            &SimpleDataStore::new()
        ),
        vec![
            control(1, SEQ_FUNCTION_ID, &[W2, W3]),
            module(2, port(10)),
            module(3, port(11)),
            source[3].clone(),
            cached(module(10, var(9000))),
            cached(module(11, var(9001))),
            control(W2, WITH_FUNCTION_ID, &[G2_10, 2]),
            control(G2_10, PARALLEL_FUNCTION_ID, &[10]),
            control(W3, WITH_FUNCTION_ID, &[G3_11, 3]),
            control(G3_11, PARALLEL_FUNCTION_ID, &[11]),
        ]
    );
}

/// Two edges to one reader are distinct occurrences, even when both have
/// the same parent. Their common ancestor is the parent, not the reader.
#[test]
fn repeated_child_edges_anchor_before_their_common_parent() {
    let source = vec![
        control(1, SEQ_FUNCTION_ID, &[2, 2]),
        module(2, port(10)),
        module(10, var(9000)),
    ];
    let bridge = bridge(&source);
    assert_eq!(
        place(
            source,
            PlacementScope::Readers,
            &bridge,
            &SimpleDataStore::new()
        ),
        vec![
            control(W1, WITH_FUNCTION_ID, &[G1_10, 1]),
            control(1, SEQ_FUNCTION_ID, &[2, 2]),
            module(2, port(10)),
            cached(module(10, var(9000))),
            control(G1_10, PARALLEL_FUNCTION_ID, &[10])
        ]
    );
}

fn attached_ids(nodes: &[Node]) -> std::collections::HashSet<Uuid> {
    let mut attached = std::collections::HashSet::new();
    let mut stack = vec![nodes[0].id];
    while let Some(id) = stack.pop() {
        if attached.insert(id) {
            stack.extend(
                nodes
                    .iter()
                    .find(|node| node.id == id)
                    .unwrap()
                    .children
                    .iter()
                    .flatten(),
            );
        }
    }
    attached
}
fn calls_to(bridge: &Bridge, function: u128) -> usize {
    bridge
        .calls
        .iter()
        .filter(|(f, _)| *f == id(function))
        .count()
}
fn get(store: &SimpleDataStore, key: &str) -> Option<Value> {
    store.slot(&Key::from(key)).get()
}
fn add_call(x: Expression) -> Expression {
    Expression::Call(Box::new(CallExpression {
        module: Expression::Uuid(MODULE),
        function: Expression::Uuid(id(29999)),
        arguments: vec![(Expression::Uuid(X), x), (Expression::Uuid(Y), lit(0.0))],
    }))
}

/// Attached writes, placed writes, and loose controls may all retain references
/// to an unused detached port. Keeping it must neither load-fail nor evaluate it.
#[test]
fn unused_detached_ports_remain_valid_without_being_evaluated() {
    for kind in 0..3 {
        let mut source = vec![
            control(1, SEQ_FUNCTION_ID, &[2]),
            module(2, port(10)),
            module(10, var(9000)),
            module(11, lit(0.0)),
        ];
        match kind {
            0 => {
                source[1].arguments.insert(_RET_PARAM_ID, parameter(11, X));
            }
            1 => {
                source[2].arguments.insert(_RET_PARAM_ID, parameter(11, X));
            }
            _ => source.push(node(
                5,
                EQUAL_FUNCTION_ID,
                vec![(EQUAL_A_PARAM_ID, port(11)), (EQUAL_B_PARAM_ID, lit(1.0))],
            )),
        }
        for scope in [PlacementScope::Readers, PlacementScope::Root] {
            let stores = [SimpleDataStore::new(), SimpleDataStore::new()];
            let mut results = Vec::new();
            for (mode, store) in stores.iter().enumerate() {
                put(store, "input/a", 1.0);
                let mut b = bridge(&source);
                let file = if mode == 0 {
                    source.clone()
                } else {
                    let placed = place(source.clone(), scope, &b, store);
                    assert_eq!(placed.iter().find(|n| n.id == id(11)), Some(&source[3]));
                    assert!(!attached_ids(&placed).contains(&id(11)));
                    placed
                };
                let tree = lower(file, &mut b, store, mode != 0);
                for _ in 0..4 {
                    assert_eq!(tick(&tree, &mut b), Status::Success);
                    assert_eq!(calls_to(&b, 20011), 0);
                }
                results.push((value(&tree, 2, X), value(&tree, 11, X), store.snapshot()));
            }
            assert_eq!(results[0], results[1], "case {kind}, {scope:?}");
        }
    }
}

/// A nested call keeps its owner detached and argument-cached, including a
/// selection around that call and dependencies crossing in either direction.
#[test]
fn call_arguments_stay_detached_and_interoperate_with_placed_ports() {
    for direction in 0..6 {
        let mut ten = module(10, add_call(var(9000)));
        let mut source = vec![control(1, SEQ_FUNCTION_ID, &[2]), module(2, port(10))];
        match direction {
            0 => {}
            1 => {
                source[1] = module(2, port(11));
                source.push(module(11, port(10)));
            }
            2..=4 => {
                ten.arguments.insert(X, add_call(port(11)));
                if direction == 3 {
                    source[1] = module(2, port(12));
                    source.push(module(12, port(10)));
                }
                if direction == 4 {
                    ten.arguments.insert(_RET_PARAM_ID, var(9004));
                    source[1] = module(2, lit(0.0));
                }
                source.push(module(11, var(9000)));
            }
            _ => {
                ten.arguments.insert(
                    X,
                    Expression::Select {
                        source: Box::new(add_call(var(9000))),
                        path: Key::new(""),
                    },
                );
            }
        }
        source.push(ten);
        for scope in [PlacementScope::Readers, PlacementScope::Root] {
            let stores = [SimpleDataStore::new(), SimpleDataStore::new()];
            let mut bridges = [bridge(&source), bridge(&source)];
            for b in &mut bridges {
                b.ops.insert(id(29999), Op::Add);
            }
            let file = place(source.clone(), scope, &bridges[1], &stores[1]);
            let attached = attached_ids(&file);
            assert!(!attached.contains(&id(10)));
            if (1..=4).contains(&direction) {
                assert!(attached.contains(&id(11)));
            }
            let trees = [
                lower(source.clone(), &mut bridges[0], &stores[0], false),
                lower(file, &mut bridges[1], &stores[1], true),
            ];
            for step in 0..24 {
                for store in &stores {
                    put(store, "input/a", (step / 8) as f64);
                }
                assert_eq!(
                    tick(&trees[0], &mut bridges[0]),
                    tick(&trees[1], &mut bridges[1])
                );
                assert_eq!(
                    value(&trees[0], 2, X),
                    value(&trees[1], 2, X),
                    "direction {direction}, step {step}"
                );
                assert_eq!(stores[0].snapshot(), stores[1].snapshot());
                assert_eq!(calls_to(&bridges[0], 20010), calls_to(&bridges[1], 20010));
                if step % 8 >= 3 {
                    assert_eq!(calls_to(&bridges[1], 20010), 0);
                }
            }
        }
    }
}

/// Two sinks sharing a key retain distinct ports, and their module calls stop
/// after settling even though their native publications keep overwriting it.
#[test]
fn sinks_sharing_a_key_keep_private_ports_and_settle() {
    let mut s10 = module(10, var(9000));
    s10.arguments.insert(_RET_PARAM_ID, var(9004));
    let mut s11 = module(11, var(9001));
    s11.arguments.insert(_RET_PARAM_ID, var(9004));
    let source = vec![
        control(1, SEQ_FUNCTION_ID, &[2]),
        module(2, port(10)),
        s10,
        s11,
    ];
    for scope in [
        None,
        Some(PlacementScope::Readers),
        Some(PlacementScope::Root),
    ] {
        let store = SimpleDataStore::new();
        put(&store, "input/a", 1.0);
        put(&store, "input/b", 100.0);
        let mut b = bridge(&source);
        let file = scope.map_or_else(|| source.clone(), |s| place(source.clone(), s, &b, &store));
        let tree = lower(file, &mut b, &store, scope.is_some());
        for step in 0..10 {
            tick(&tree, &mut b);
            assert_eq!(value(&tree, 2, X), Some(Value::F64(3.0)));
            assert_eq!(value(&tree, 10, _RET_PARAM_ID), Some(Value::F64(3.0)));
            assert_eq!(value(&tree, 11, _RET_PARAM_ID), Some(Value::F64(102.0)));
            assert_eq!(get(&store, "out/a"), Some(Value::F64(102.0)));
            if step >= 3 {
                assert_eq!(calls_to(&b, 20010) + calls_to(&b, 20011), 0);
            }
        }
    }
}

/// Publication order remains intentional: the tree's 100 wins after placed
/// publication; detached publication writes 3 after the body. Neither form
/// calls the sink's module on settled ticks when the body overwrites its key.
#[test]
fn tree_writes_win_after_placed_sinks_without_recalling_the_sink() {
    let mut sink = module(10, var(9000));
    sink.arguments.insert(_RET_PARAM_ID, var(9004));
    let writer = node(
        2,
        id(20002),
        vec![(X, var(9001)), (Y, lit(0.0)), (_RET_PARAM_ID, var(9004))],
    );
    let source = vec![control(1, SEQ_FUNCTION_ID, &[2]), writer, sink];
    for scope in [
        None,
        Some(PlacementScope::Readers),
        Some(PlacementScope::Root),
    ] {
        let store = SimpleDataStore::new();
        put(&store, "input/a", 1.0);
        put(&store, "input/b", 100.0);
        let mut b = bridge(&source);
        let file = scope.map_or_else(|| source.clone(), |s| place(source.clone(), s, &b, &store));
        let tree = lower(file, &mut b, &store, scope.is_some());
        for step in 0..10 {
            tick(&tree, &mut b);
            assert_eq!(
                get(&store, "out/a"),
                Some(Value::F64(if scope.is_some() { 100.0 } else { 3.0 }))
            );
            if step >= 3 {
                assert_eq!(calls_to(&b, 20010), 0);
            }
        }
    }
}

/// A single child cannot supply both a prerequisite and the WITH body.
#[test]
fn with_one_child_is_an_error() {
    for incremental in [false, true] {
        let mut b = Bridge::default();
        let tree = lower(
            vec![
                control(1, WITH_FUNCTION_ID, &[2]),
                node(2, SUCCEED_FUNCTION_ID, vec![]),
            ],
            &mut b,
            &SimpleDataStore::new(),
            incremental,
        );
        assert!(tree
            .tick(&mut b)
            .unwrap_err()
            .to_string()
            .contains("at least two"));
    }
}

/// An explicitly empty child list is still a detached leaf; attachment clears
/// it so the module receives scalar arguments rather than a children array.
#[test]
fn empty_children_on_a_detached_node_place_as_a_leaf() {
    let mut ten = module(10, var(9000));
    ten.children = Some(vec![]);
    let source = vec![control(1, SEQ_FUNCTION_ID, &[2]), module(2, port(10)), ten];
    let store = SimpleDataStore::new();
    put(&store, "input/a", 1.0);
    let mut b = bridge(&source);
    let file = place(source, PlacementScope::Readers, &b, &store);
    assert!(file
        .iter()
        .find(|n| n.id == id(10))
        .unwrap()
        .children
        .is_none());
    let tree = lower(file, &mut b, &store, true);
    assert_eq!(tick(&tree, &mut b), Status::Success);
    assert_eq!(value(&tree, 2, X), Some(Value::F64(3.0)));
}

/// Existing file variables reserve their ids against implicit generated ports;
/// a later attached write must not overwrite the placed computation's port.
#[test]
fn generated_port_ids_do_not_alias_file_variables() {
    let generated = 0x504f5200000040008000000000000000 ^ 10;
    let mut ten = module(10, var(9000));
    ten.arguments.remove(&_RET_PARAM_ID);
    let source = vec![
        control(1, SEQ_FUNCTION_ID, &[2, 3]),
        module(2, port(10)),
        node(
            3,
            id(20003),
            vec![
                (X, lit(500.0)),
                (Y, lit(0.0)),
                (_RET_PARAM_ID, var(generated)),
            ],
        ),
        ten,
    ];
    let store = SimpleDataStore::new();
    put(&store, "input/a", 1.0);
    let mut b = bridge(&source);
    let file = place(source, PlacementScope::Readers, &b, &store);
    assert_ne!(
        file.iter().find(|n| n.id == id(10)).unwrap().arguments[&_RET_PARAM_ID],
        var(generated)
    );
    let tree = lower(file, &mut b, &store, true);
    for _ in 0..4 {
        tick(&tree, &mut b);
        assert_eq!(value(&tree, 2, X), Some(Value::F64(3.0)));
        assert_eq!(value(&tree, 3, _RET_PARAM_ID), Some(Value::F64(500.0)));
    }
}

/// First-listed groups execute first, observable through shared-key writes
/// and the ordered module calls. Reversing the file reverses the winner.
#[test]
fn with_prerequisites_run_in_first_listed_group_order() {
    for reverse in [false, true] {
        let mut a = module(10, var(9000));
        a.arguments.insert(_RET_PARAM_ID, var(9004));
        let mut b = module(11, var(9001));
        b.arguments.insert(_RET_PARAM_ID, var(9004));
        let source = if reverse {
            vec![node(1, SUCCEED_FUNCTION_ID, vec![]), b, a]
        } else {
            vec![node(1, SUCCEED_FUNCTION_ID, vec![]), a, b]
        };
        let store = SimpleDataStore::new();
        put(&store, "input/a", 1.0);
        put(&store, "input/b", 100.0);
        let mut b = bridge(&source);
        let file = place(source, PlacementScope::Readers, &b, &store);
        let tree = lower(file, &mut b, &store, true);
        tick(&tree, &mut b);
        assert_eq!(
            b.calls
                .iter()
                .map(|(id, _)| id.as_u128())
                .collect::<Vec<_>>(),
            if reverse {
                vec![20011, 20010]
            } else {
                vec![20010, 20011]
            }
        );
        assert_eq!(
            get(&store, "out/a"),
            Some(Value::F64(if reverse { 3.0 } else { 102.0 }))
        );
    }
}

/// Mutable sink ports read the live key before computing, then publish their
/// new private state.
#[test]
fn mutable_sink_ports_start_from_the_key_and_keep_their_own_state() {
    let mut sink = module(10, var(9000));
    sink.arguments.insert(STATE, var(9004));
    let source = vec![
        control(1, SEQ_FUNCTION_ID, &[2]),
        module(2, parameter(10, STATE)),
        sink,
    ];
    let stores = [SimpleDataStore::new(), SimpleDataStore::new()];
    let mut bridges = [bridge(&source), bridge(&source)];
    for b in &mut bridges {
        b.ops.insert(id(20010), Op::Approach);
    }
    let file = place(
        source.clone(),
        PlacementScope::Readers,
        &bridges[1],
        &stores[1],
    );
    let trees = [
        lower(source, &mut bridges[0], &stores[0], false),
        lower(file, &mut bridges[1], &stores[1], true),
    ];
    for step in 0..24 {
        for store in &stores {
            put(store, "input/a", 8.0);
            if step % 8 == 0 {
                put(store, "out/a", (step + 2) as f64);
            }
        }
        assert_eq!(
            tick(&trees[0], &mut bridges[0]),
            tick(&trees[1], &mut bridges[1])
        );
        assert_eq!(value(&trees[0], 2, X), value(&trees[1], 2, X));
        assert_eq!(stores[0].snapshot(), stores[1].snapshot());
    }
}

/// Copy preserves absence and a present Unit separately, and restores a
/// foreign overwrite without a module call in either execution mode.
#[test]
fn native_copy_preserves_presence_and_republishes_without_module_calls() {
    for incremental in [false, true] {
        let store = SimpleDataStore::new();
        let mut b = Bridge::default();
        let tree = lower(
            vec![node(
                1,
                COPY_FUNCTION_ID,
                vec![
                    (COPY_VALUE_PARAM_ID, var(9000)),
                    (COPY_OUT_PARAM_ID, var(9004)),
                ],
            )],
            &mut b,
            &store,
            incremental,
        );
        for value in [None, Some(Value::Unit), Some(Value::F64(3.0))] {
            store
                .slot(&Key::from("input/a"))
                .set(value.clone())
                .unwrap();
            for _ in 0..3 {
                put(&store, "out/a", 99.0);
                assert_eq!(tick(&tree, &mut b), Status::Success);
                assert_eq!(get(&store, "out/a"), value);
                assert!(b.calls.is_empty());
            }
        }
    }
}

/// Publication follows file order across interleaved components and reverse
/// dependency order, after all sink computations, as detached publication does.
#[test]
fn placed_sink_publication_keeps_file_order_across_components() {
    let mut a = module(10, var(9000));
    a.arguments.insert(_RET_PARAM_ID, var(9004));
    let mut b = module(11, port(10));
    b.arguments.insert(_RET_PARAM_ID, var(9004));
    let mut c = module(12, var(9001));
    c.arguments.insert(_RET_PARAM_ID, var(9004));
    for sinks in [vec![b.clone(), a.clone()], vec![a.clone(), c, b]] {
        let source = std::iter::once(node(1, SUCCEED_FUNCTION_ID, vec![]))
            .chain(sinks)
            .collect::<Vec<_>>();
        let mut results = Vec::new();
        for scope in [
            None,
            Some(PlacementScope::Readers),
            Some(PlacementScope::Root),
        ] {
            let store = SimpleDataStore::new();
            put(&store, "input/a", 1.0);
            put(&store, "input/b", 100.0);
            let mut b = bridge(&source);
            let file =
                scope.map_or_else(|| source.clone(), |s| place(source.clone(), s, &b, &store));
            let tree = lower(file, &mut b, &store, scope.is_some());
            let mut values = Vec::new();
            for step in 0..10 {
                tick(&tree, &mut b);
                values.push(get(&store, "out/a"));
                if step >= 3 {
                    assert!(b.calls.is_empty());
                }
            }
            results.push(values);
        }
        assert_eq!(results[0], results[1]);
        assert_eq!(results[0], results[2]);
    }
}

/// Tick the detached file and its placement under each scope in lockstep, each
/// from its own store holding `keys`, and check after every tick that each
/// form observes `expected`, the detached form included.
fn in_lockstep<T: PartialEq + std::fmt::Debug>(
    source: &[Node],
    ops: &[(u128, Op)],
    keys: &[(&str, f64)],
    expected: T,
    observe: impl Fn(&LoweredTree, &SimpleDataStore) -> T,
) {
    let scopes = [
        None,
        Some(PlacementScope::Readers),
        Some(PlacementScope::Root),
    ];
    let stores = scopes.map(|_| SimpleDataStore::new());
    let mut bridges = scopes.map(|_| {
        let mut b = bridge(source);
        for (function, op) in ops {
            b.ops.insert(id(*function), *op);
        }
        b
    });
    let trees: Vec<_> = scopes
        .iter()
        .zip(&stores)
        .zip(&mut bridges)
        .map(|((scope, store), b)| {
            for (key, x) in keys {
                put(store, key, *x);
            }
            let file =
                scope.map_or_else(|| source.to_vec(), |s| place(source.to_vec(), s, b, store));
            lower(file, b, store, scope.is_some())
        })
        .collect();
    for step in 0..6 {
        for (at, scope) in scopes.iter().enumerate() {
            tick(&trees[at], &mut bridges[at]);
            assert_eq!(
                observe(&trees[at], &stores[at]),
                expected,
                "{scope:?}, tick {step}"
            );
        }
        assert_eq!(stores[0].snapshot(), stores[1].snapshot(), "tick {step}");
        assert_eq!(stores[0].snapshot(), stores[2].snapshot(), "tick {step}");
    }
}

/// A return bound to a detached reader's input that names a store key feeds
/// that input alone, so the key keeps its value once both nodes are placed.
#[test]
fn a_return_feeding_a_detached_input_leaves_its_key_unwritten() {
    let source = vec![
        module(1, port(11)),
        node(
            11,
            id(20011),
            vec![(X, var(9001)), (Y, lit(0.0)), (_RET_PARAM_ID, var(30011))],
        ),
        node(
            10,
            id(20010),
            vec![
                (X, lit(1.0)),
                (Y, lit(2.0)),
                (_RET_PARAM_ID, parameter(11, X)),
            ],
        ),
    ];
    in_lockstep(
        &source,
        &[],
        &[("input/b", 99.0)],
        [Some(99.0), Some(3.0), Some(3.0)].map(|x| x.map(Value::F64)),
        |tree, store| [get(store, "input/b"), value(tree, 11, X), value(tree, 1, X)],
    );
}

/// A sink publishes its return, then its mutable parameters in signature
/// order: of two outputs bound to one key, the parameter's value stands.
#[test]
fn a_placed_sink_publishes_its_outputs_in_detached_order() {
    let source = vec![
        control(1, SEQ_FUNCTION_ID, &[2, 3]),
        module(2, port(10)),
        module(3, parameter(10, STATE)),
        node(
            10,
            id(20010),
            vec![
                (X, lit(3.0)),
                (Y, lit(7.0)),
                (STATE, var(9004)),
                (_RET_PARAM_ID, var(9004)),
            ],
        ),
    ];
    in_lockstep(
        &source,
        &[(20010, Op::Split)],
        &[("out/a", 0.0)],
        [Some(7.0), Some(3.0), Some(7.0)].map(|x| x.map(Value::F64)),
        |tree, store| [get(store, "out/a"), value(tree, 2, X), value(tree, 3, X)],
    );
}

/// Every sink of a key that a detached sink publishes stays detached, and so,
/// in turn, does every sink of a key one of those publishes: all publishers of
/// one key publish after the tree, and the later listed wins, as detached.
#[test]
fn sinks_sharing_a_key_with_a_detached_sink_stay_detached() {
    // `out/a`: 10 (with a call), then 11 through a second variable of that
    // name. `out/b`: 11's `OUT`, then 12.
    let source = vec![
        node(1, SUCCEED_FUNCTION_ID, vec![]),
        node(
            10,
            id(20010),
            vec![
                (X, add_call(lit(1.0))),
                (Y, lit(2.0)),
                (_RET_PARAM_ID, var(9004)),
            ],
        ),
        node(
            11,
            id(20011),
            vec![
                (X, lit(5.0)),
                (Y, lit(2.0)),
                (OUT, var(9005)),
                (_RET_PARAM_ID, var(9006)),
            ],
        ),
        node(
            12,
            id(20012),
            vec![(X, lit(4.0)), (Y, lit(5.0)), (_RET_PARAM_ID, var(9005))],
        ),
    ];
    in_lockstep(
        &source,
        &[(29999, Op::Add)],
        &[("out/a", 0.0), ("out/b", 0.0)],
        [Some(7.0), Some(9.0), Some(3.0), Some(7.0), Some(9.0)].map(|x| x.map(Value::F64)),
        |tree, store| {
            [
                get(store, "out/a"),
                get(store, "out/b"),
                value(tree, 10, _RET_PARAM_ID),
                value(tree, 11, _RET_PARAM_ID),
                value(tree, 12, _RET_PARAM_ID),
            ]
        },
    );
}
