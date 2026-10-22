//! Incremental execution checks and the existing-suite opt-in helper.
pub(crate) fn existing_tests_enabled() -> bool {
    std::env::var_os("ARORA_TEST_INCREMENTAL").is_some()
}

#[path = "../benches/support/incremental.rs"]
mod fixture;

use std::collections::{HashMap, HashSet};

use arora_behavior::Status;
use arora_simple_data_store::SimpleDataStore;
use arora_types::data::{DataStore, Key};
use arora_types::value::Value;
use uuid::Uuid;

use crate::nodes::*;
use crate::schema::{Expression, Node, NodeParameterId, _ON_CHANGE_PARAM_ID, _RET_PARAM_ID};
use crate::{load_behavior_tree_nodes_with, lower_behavior_tree, LoweredTree};
use fixture::{Bridge, Op, OUT, STATE, X, Y};

fn id(n: u128) -> Uuid {
    Uuid::from_u128(n)
}
fn lit(n: f64) -> Expression {
    Expression::Value(Value::F64(n))
}
fn variable(n: u128) -> Expression {
    Expression::VariableId(id(n))
}
fn port(node: u128, parameter: Uuid) -> Expression {
    Expression::NodeArgument(NodeParameterId {
        node: id(node),
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
fn control(n: u128, function: Uuid, children: Vec<u128>) -> Node {
    let mut node = node(n, function, vec![]);
    node.children = Some(children.into_iter().map(id).collect());
    if function == SEQ_STAR_FUNCTION_ID {
        node.arguments.insert(
            SEQ_STAR_CURRENT_INDEX_PARAM_ID,
            Expression::Value(Value::U16(0)),
        );
    }
    if function == RUN_STATUS_FUNCTION_ID {
        node.arguments
            .insert(RUN_STATUS_LATCH_PARAM_ID, Expression::Value(Value::Unit));
        node.arguments
            .insert(RUN_STATUS_OUT_PARAM_ID, variable(8000 + n));
    }
    node
}
fn module(n: u128, input: Expression, on_change: bool) -> Node {
    let mut node = node(n, id(20000 + n), vec![(X, input), (Y, lit(2.0))]);
    if on_change {
        node.arguments
            .insert(_ON_CHANGE_PARAM_ID, Expression::Value(Value::Boolean(true)));
    }
    node
}
fn put(store: &SimpleDataStore, name: &str, value: Option<Value>) {
    store.slot(&Key::from(name)).set(value).unwrap();
}
fn lower(
    nodes: Vec<Node>,
    bridge: &mut Bridge,
    store: &SimpleDataStore,
    incremental: bool,
) -> LoweredTree {
    let names = HashMap::from([
        (id(9000), "a".into()),
        (id(9001), "b".into()),
        (id(9002), "c".into()),
        (id(9003), "d".into()),
        (id(9010), "a".into()),
        (id(9004), "out".into()),
        (id(9005), "y".into()),
        (id(9006), "seen".into()),
    ]);
    let tree =
        load_behavior_tree_nodes_with(nodes, &|name| Some(store.slot(&Key::from(name))), &names)
            .unwrap();
    let key_store = store.clone();
    let tree = tree.with_key_resolver(std::rc::Rc::new(move |name| {
        Some(key_store.slot(&Key::from(name)))
    }));
    let tree = lower_behavior_tree(tree, &bridge.index(), bridge, false).unwrap();
    tree.set_incremental(incremental);
    tree
}
/// Tick once, leaving in `bridge.visits` the nodes the tick ran rather than
/// answered from their caches.
fn tick(tree: &LoweredTree, bridge: &mut Bridge) -> Status {
    bridge.reset_counts();
    let before = tree.executed_visits();
    let status = tree.tick(bridge).unwrap();
    bridge.visits = tree.executed_visits() - before;
    status
}

#[test]
fn a_settled_tree_ticks_no_node() {
    let mut bridge = Bridge::default();
    let tree = lower(
        vec![
            control(1, SEQ_FUNCTION_ID, vec![2, 3]),
            node(2, SUCCEED_FUNCTION_ID, vec![]),
            node(3, SUCCEED_FUNCTION_ID, vec![]),
        ],
        &mut bridge,
        &SimpleDataStore::new(),
        true,
    );
    assert_eq!(tick(&tree, &mut bridge), Status::Success);
    assert_eq!((bridge.visits, bridge.dispatches), (3, 1));
    // A clean root answers from its cache before any engine dispatch.
    assert_eq!(tick(&tree, &mut bridge), Status::Success);
    assert_eq!((bridge.visits, bridge.dispatches), (0, 0));
}

#[test]
fn a_changed_store_key_ticks_only_its_readers_and_ancestors() {
    let mut bridge = Bridge::default();
    let store = SimpleDataStore::new();
    let equal = |n, key| {
        node(
            n,
            EQUAL_FUNCTION_ID,
            vec![
                (EQUAL_A_PARAM_ID, variable(key)),
                (EQUAL_B_PARAM_ID, lit(1.0)),
            ],
        )
    };
    let tree = lower(
        vec![
            control(1, PARALLEL_FUNCTION_ID, vec![2, 3]),
            equal(2, 9000),
            equal(3, 9001),
        ],
        &mut bridge,
        &store,
        true,
    );
    tick(&tree, &mut bridge);
    put(&store, "a", Some(Value::F64(1.0)));
    tick(&tree, &mut bridge);
    assert_eq!(bridge.visits, 2);
    tick(&tree, &mut bridge);
    assert_eq!(bridge.visits, 0);
    put(&store, "b", Some(Value::F64(1.0)));
    assert_eq!(tick(&tree, &mut bridge), Status::Success);
    assert_eq!(bridge.visits, 2);
}

#[test]
fn a_running_leaf_and_its_ancestors_are_polled_every_tick() {
    let mut leaf = module(2, lit(1.0), true);
    leaf.arguments.insert(STATE, lit(0.0));
    let mut pair = Lockstep::new(
        vec![control(1, SEQ_FUNCTION_ID, vec![2]), leaf],
        HashMap::from([(id(20002), Op::Running(4))]),
    );
    for step in 0..12 {
        let expected = if step % 5 == 4 {
            Status::Success
        } else {
            Status::Running
        };
        assert_eq!(pair.tick(), expected);
        assert_eq!(
            (
                pair.incremental_bridge.visits,
                pair.incremental_bridge.calls.len()
            ),
            (2, 1)
        );
    }
}

#[test]
fn a_module_without_on_change_is_called_every_visit() {
    let mut bridge = Bridge::default();
    bridge.ops.insert(id(20002), Op::Effect);
    let mut leaf = module(2, lit(1.0), false);
    leaf.arguments.insert(STATE, lit(0.0));
    let tree = lower(
        vec![control(1, SEQ_FUNCTION_ID, vec![2]), leaf],
        &mut bridge,
        &SimpleDataStore::new(),
        true,
    );
    for _ in 0..8 {
        assert_eq!(tick(&tree, &mut bridge), Status::Success);
        assert_eq!(bridge.calls.len(), 1);
    }
    assert_eq!(
        tree.tree().node_arg_variables[&NodeParameterId {
            node: id(2),
            parameter: STATE
        }]
            .get(),
        Some(Value::F64(8.0))
    );
}

#[test]
fn on_change_calls_only_when_a_read_changes_and_keeps_its_output() {
    let store = SimpleDataStore::new();
    put(&store, "a", Some(Value::F64(1.0)));
    let mut bridge = Bridge::default();
    bridge.ops.insert(id(20001), Op::Add);
    let mut leaf = module(1, variable(9000), true);
    leaf.arguments.insert(_RET_PARAM_ID, variable(8001));
    let tree = lower(vec![leaf], &mut bridge, &store, true);
    tick(&tree, &mut bridge);
    tick(&tree, &mut bridge);
    assert!(bridge.calls.is_empty());
    put(&store, "a", Some(Value::F64(3.0)));
    tick(&tree, &mut bridge);
    assert_eq!(bridge.calls.len(), 1);
    assert_eq!(
        tree.tree().variables.borrow()[&id(8001)].get(),
        Some(Value::F64(5.0))
    );
}

/// An output overwrite invalidates its cached producer, including when a
/// sibling writes within the tick; the producer's own write lets it settle.
#[test]
fn an_overwritten_output_is_restored_by_its_cached_writer() {
    for case in ["store", "later sibling", "earlier sibling"] {
        let naive_store = SimpleDataStore::new();
        let incremental_store = SimpleDataStore::new();
        for store in [&naive_store, &incremental_store] {
            put(store, "y", Some(Value::F64(0.0)));
        }
        let nodes = || {
            let mut leaf = module(2, lit(1.0), true);
            leaf.arguments.insert(_RET_PARAM_ID, variable(9005));
            let mut reader = module(3, variable(9005), false);
            reader.arguments.insert(Y, lit(0.0));
            reader.arguments.insert(_RET_PARAM_ID, variable(9006));
            let children = match case {
                "store" => vec![2, 3],
                "later sibling" => vec![2, 3, 4],
                _ => vec![4, 2, 3],
            };
            let mut nodes = vec![control(1, SEQ_FUNCTION_ID, children), leaf, reader];
            if case != "store" {
                let mut writer = module(4, lit(7.0), false);
                writer.arguments.insert(OUT, variable(9005));
                writer.arguments.insert(_RET_PARAM_ID, variable(8004));
                nodes.push(writer);
            }
            nodes
        };
        let mut naive_bridge = Bridge::default();
        let mut incremental_bridge = Bridge::default();
        for bridge in [&mut naive_bridge, &mut incremental_bridge] {
            for n in 2..=4 {
                bridge.ops.insert(id(20000 + n), Op::Add);
            }
        }
        let naive = lower(nodes(), &mut naive_bridge, &naive_store, false);
        let incremental = lower(nodes(), &mut incremental_bridge, &incremental_store, true);
        for step in 0..6 {
            if case == "store" && step == 2 {
                put(&naive_store, "y", Some(Value::F64(9.0)));
                put(&incremental_store, "y", Some(Value::F64(9.0)));
            }
            let expected = tick(&naive, &mut naive_bridge);
            assert_eq!(
                tick(&incremental, &mut incremental_bridge),
                expected,
                "{case}, tick {step}"
            );
            assert_eq!(expected, Status::Success);
            for name in ["y", "seen"] {
                let expected = naive_store.slot(&Key::from(name)).get();
                assert_eq!(
                    incremental_store.slot(&Key::from(name)).get(),
                    expected,
                    "{case}, tick {step}, key {name}"
                );
                let value = if name == "y" && case == "later sibling" {
                    9.0
                } else {
                    3.0
                };
                assert_eq!(expected, Some(Value::F64(value)));
            }
            let calls = incremental_bridge
                .calls
                .iter()
                .filter(|(function, _)| *function == id(20002))
                .count();
            let expected_calls = usize::from(case != "store" || step == 0 || step == 2);
            assert_eq!(calls, expected_calls, "{case}, tick {step}");
        }
    }
}

#[test]
fn a_reactive_sequence_rechecks_an_earlier_condition_while_a_later_child_runs() {
    let store = SimpleDataStore::new();
    put(&store, "a", Some(Value::F64(1.0)));
    let mut bridge = Bridge::default();
    let tree = lower(
        vec![
            control(1, SEQ_FUNCTION_ID, vec![2, 3]),
            node(
                2,
                EQUAL_FUNCTION_ID,
                vec![
                    (EQUAL_A_PARAM_ID, variable(9000)),
                    (EQUAL_B_PARAM_ID, lit(1.0)),
                ],
            ),
            node(3, RUN_FUNCTION_ID, vec![]),
        ],
        &mut bridge,
        &store,
        true,
    );
    assert_eq!(tick(&tree, &mut bridge), Status::Running);
    put(&store, "a", Some(Value::F64(2.0)));
    assert_eq!(tick(&tree, &mut bridge), Status::Failure);
    assert_eq!(bridge.visits, 2);
}

#[test]
fn a_later_write_reaches_an_earlier_reader_on_the_next_tick() {
    let mut bridge = Bridge::default();
    bridge.ops.insert(id(20003), Op::Add);
    let mut writer = module(3, lit(1.0), true);
    writer.arguments.insert(_RET_PARAM_ID, variable(8000));
    let tree = lower(
        vec![
            control(1, PARALLEL_FUNCTION_ID, vec![2, 3]),
            node(
                2,
                EQUAL_FUNCTION_ID,
                vec![
                    (EQUAL_A_PARAM_ID, variable(8000)),
                    (EQUAL_B_PARAM_ID, lit(3.0)),
                ],
            ),
            writer,
        ],
        &mut bridge,
        &SimpleDataStore::new(),
        true,
    );
    assert_eq!(tick(&tree, &mut bridge), Status::Failure);
    assert_eq!(tick(&tree, &mut bridge), Status::Success);
    assert_eq!(bridge.visits, 2);
    tick(&tree, &mut bridge);
    assert_eq!(bridge.visits, 0);
}

#[test]
fn detached_rewrites_dirty_readers_only_when_the_output_changes() {
    let mut bridge = Bridge::default();
    bridge.ops.insert(id(20003), Op::Clamp);
    let store = SimpleDataStore::new();
    put(&store, "a", Some(Value::F64(10.0)));
    let mut detached = module(3, variable(9000), false);
    detached.arguments.insert(_RET_PARAM_ID, variable(9004));
    let tree = lower(
        vec![
            control(1, SEQ_FUNCTION_ID, vec![2]),
            node(
                2,
                EQUAL_FUNCTION_ID,
                vec![
                    (EQUAL_A_PARAM_ID, port(3, _RET_PARAM_ID)),
                    (EQUAL_B_PARAM_ID, lit(3.0)),
                ],
            ),
            detached,
        ],
        &mut bridge,
        &store,
        true,
    );
    assert_eq!(tick(&tree, &mut bridge), Status::Success);
    tick(&tree, &mut bridge);
    assert_eq!(
        bridge.visits, 1,
        "ancestor visits so the reader can pull, then skip"
    );
    assert!(bridge.calls.is_empty());
    put(&store, "a", Some(Value::F64(11.0)));
    tick(&tree, &mut bridge);
    assert_eq!(
        (bridge.visits, bridge.calls.len()),
        (1, 1),
        "changed arguments but equal clamped output"
    );
    put(&store, "a", Some(Value::F64(1.0)));
    assert_eq!(tick(&tree, &mut bridge), Status::Failure);
    assert_eq!((bridge.visits, bridge.calls.len()), (2, 1));
}

#[test]
fn a_shared_node_has_a_cache_for_each_parent_path() {
    let mut bridge = Bridge::default();
    let store = SimpleDataStore::new();
    let tree = lower(
        vec![
            control(1, PARALLEL_FUNCTION_ID, vec![2, 3]),
            control(2, SEQ_FUNCTION_ID, vec![4]),
            control(3, SEQ_FUNCTION_ID, vec![4]),
            node(
                4,
                EQUAL_FUNCTION_ID,
                vec![
                    (EQUAL_A_PARAM_ID, variable(9000)),
                    (EQUAL_B_PARAM_ID, lit(1.0)),
                ],
            ),
        ],
        &mut bridge,
        &store,
        true,
    );
    tick(&tree, &mut bridge);
    assert_eq!(bridge.visits, 5);
    tick(&tree, &mut bridge);
    assert_eq!(bridge.visits, 0);
    put(&store, "a", Some(Value::F64(1.0)));
    assert_eq!(tick(&tree, &mut bridge), Status::Success);
    assert_eq!(bridge.visits, 5);
}

#[test]
fn relowering_an_edit_starts_dirty_and_keeps_the_result_correct() {
    let mut bridge = Bridge::default();
    let store = SimpleDataStore::new();
    let tree = lower(
        vec![node(1, SUCCEED_FUNCTION_ID, vec![])],
        &mut bridge,
        &store,
        true,
    );
    tick(&tree, &mut bridge);
    tick(&tree, &mut bridge);
    assert_eq!(bridge.visits, 0);
    tree.unregister(&mut bridge);
    let edited = lower(
        vec![node(1, FAIL_FUNCTION_ID, vec![])],
        &mut bridge,
        &store,
        true,
    );
    assert_eq!(tick(&edited, &mut bridge), Status::Failure);
    assert_eq!(bridge.visits, 1);
}

#[test]
fn cell_versions_change_only_when_the_value_changes() {
    let mut bridge = Bridge::default();
    let tree = lower(
        vec![node(
            1,
            EQUAL_FUNCTION_ID,
            vec![
                (EQUAL_A_PARAM_ID, variable(8001)),
                (EQUAL_B_PARAM_ID, variable(9000)),
            ],
        )],
        &mut bridge,
        &SimpleDataStore::new(),
        true,
    );
    let cell = tree.tree().variables.borrow()[&id(8001)].clone();
    assert_eq!(cell.version(), 0);
    cell.set(Value::F64(1.0));
    assert_eq!(cell.version(), 1);
    cell.set(Value::F64(1.0));
    assert_eq!(cell.version(), 1);
    cell.set_opt(None);
    assert_eq!(cell.version(), 2);
    let store = SimpleDataStore::new();
    let tree = lower(
        vec![node(
            1,
            EQUAL_FUNCTION_ID,
            vec![
                (EQUAL_A_PARAM_ID, variable(9000)),
                (EQUAL_B_PARAM_ID, lit(0.0)),
            ],
        )],
        &mut bridge,
        &store,
        true,
    );
    let stored = tree.tree().variables.borrow()[&id(9000)].clone();
    put(&store, "a", Some(Value::F64(2.0)));
    stored.observe();
    assert_eq!(stored.version(), 1);
    stored.set(Value::F64(2.0));
    assert_eq!(stored.version(), 1);
    put(&store, "a", Some(Value::F64(3.0)));
    stored.set(Value::F64(2.0));
    assert_eq!(
        stored.version(),
        3,
        "the outside move and its overwrite both count"
    );
}

struct Random(u64);
impl Random {
    fn next(&mut self, upper: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % upper
    }
}
struct Generator {
    random: Random,
    next: u128,
    nodes: Vec<Node>,
    ops: HashMap<Uuid, Op>,
    opted: HashSet<Uuid>,
    shareable: Vec<u128>,
    results: Vec<u128>,
}
impl Generator {
    fn build(&mut self, depth: u32) -> u128 {
        if depth > 0 && !self.shareable.is_empty() && self.random.next(12) == 0 {
            return self.shareable[self.random.next(self.shareable.len() as u64) as usize];
        }
        self.next += 1;
        let n = self.next;
        if depth < 3 && self.random.next(4) != 0 {
            let function = [
                SEQ_FUNCTION_ID,
                FALLBACK_FUNCTION_ID,
                PARALLEL_FUNCTION_ID,
                SEQ_STAR_FUNCTION_ID,
                RUN_STATUS_FUNCTION_ID,
            ][self.random.next(5) as usize];
            let count = if function == RUN_STATUS_FUNCTION_ID {
                1
            } else {
                2 + self.random.next(3)
            };
            let children = (0..count).map(|_| self.build(depth + 1)).collect();
            self.nodes.push(control(n, function, children));
        } else {
            let kind = self.random.next(10);
            let leaf = match kind {
                0..=2 => node(
                    n,
                    [SUCCEED_FUNCTION_ID, FAIL_FUNCTION_ID, RUN_FUNCTION_ID][kind as usize],
                    vec![],
                ),
                3 => node(
                    n,
                    EQUAL_FUNCTION_ID,
                    vec![
                        (
                            EQUAL_A_PARAM_ID,
                            variable([9000, 9001, 9002, 9003, 9010][self.random.next(5) as usize]),
                        ),
                        (EQUAL_B_PARAM_ID, lit(self.random.next(5) as f64)),
                    ],
                ),
                _ => {
                    let opted = kind < 7 && self.random.next(2) == 0;
                    let input = if !self.results.is_empty() && self.random.next(4) == 0 {
                        variable(
                            8000 + self.results
                                [self.random.next(self.results.len() as u64) as usize],
                        )
                    } else {
                        variable([9000, 9001, 9002, 9003, 9010][self.random.next(5) as usize])
                    };
                    let mut leaf = module(n, input.clone(), opted);
                    let mut op = match kind {
                        4 => Op::Add,
                        5 => Op::Mul,
                        6 => Op::Clamp,
                        7 => Op::Remap,
                        8 => Op::Effect,
                        _ => Op::Running(1 + self.random.next(5)),
                    };
                    if kind >= 8 {
                        leaf.arguments.insert(STATE, lit(0.0));
                    } else if opted && self.random.next(3) == 0 {
                        leaf.arguments.insert(_RET_PARAM_ID, variable(9004));
                    } else {
                        leaf.arguments.insert(_RET_PARAM_ID, variable(8000 + n));
                        self.results.push(n);
                    }
                    if opted && self.random.next(4) == 0 {
                        op = Op::Approach;
                        leaf.arguments.insert(STATE, lit(0.0));
                    }
                    if kind < 8 && self.random.next(8) == 0 {
                        leaf.arguments.insert(_RET_PARAM_ID, input);
                    }
                    // Some outputs are both tree-local and store-visible through
                    // an in-out port, so differential checks cover both writes.
                    if kind == 4 && !opted && self.random.next(3) == 0 {
                        leaf.arguments.insert(OUT, variable(9004));
                    }
                    self.ops.insert(leaf.function, op);
                    if opted {
                        self.opted.insert(leaf.function);
                    }
                    leaf
                }
            };
            self.nodes.push(leaf);
            self.shareable.push(n);
        }
        n
    }
}
fn generated(seed: u64) -> Generator {
    let mut generator = Generator {
        random: Random(seed),
        next: 0,
        nodes: Vec::new(),
        ops: HashMap::new(),
        opted: HashSet::new(),
        shareable: Vec::new(),
        results: Vec::new(),
    };
    let root = generator.build(0);
    // Even seeds visit a cached output between two ordinary writers, even when
    // the random subtree short-circuits before reaching module leaves. Odd
    // seeds keep the random root, so a settled root's cache is compared too:
    // the ordinary writers keep the wrapper dirty every tick.
    let top = if seed.is_multiple_of(2) {
        let wrapper = generator.next + 1;
        let earlier = wrapper + 1;
        let cached = wrapper + 2;
        let later = wrapper + 3;
        for n in [earlier, cached, later] {
            let opted = n == cached;
            let mut leaf = module(n, variable(9000 + (n % 4)), opted);
            if opted {
                leaf.arguments.insert(_RET_PARAM_ID, variable(9004));
                generator.opted.insert(leaf.function);
            } else {
                leaf.arguments.insert(OUT, variable(9004));
                leaf.arguments.insert(_RET_PARAM_ID, variable(8000 + n));
            }
            generator.ops.insert(leaf.function, Op::Add);
            generator.nodes.push(leaf);
        }
        generator.nodes.push(control(
            wrapper,
            PARALLEL_FUNCTION_ID,
            vec![earlier, cached, later, root],
        ));
        wrapper
    } else {
        root
    };
    // Loading takes the first listed node as the root.
    generator
        .nodes
        .sort_by_key(|node| (node.id != id(top), node.id));
    generator
}
fn cells(
    tree: &LoweredTree,
) -> (
    HashMap<Uuid, Option<Value>>,
    HashMap<NodeParameterId, Option<Value>>,
) {
    (
        tree.tree()
            .variables
            .borrow()
            .iter()
            .map(|(id, cell)| (*id, cell.get()))
            .collect(),
        tree.tree()
            .node_arg_variables
            .iter()
            .map(|(id, cell)| (id.clone(), cell.get()))
            .collect(),
    )
}

/// Replays 81,920 deterministic ticks across controls, mutable call-site state,
/// shared paths, tree-local results and overwritten cached outputs. Ordinary
/// calls must match in order; every omitted opt-in call must have unchanged
/// module arguments. Aggregate calls and node visits must show caching occurred.
#[test]
fn seeded_trees_match_the_naive_executor_in_lockstep() {
    let mut opted_calls = [0_u64; 2];
    let mut visits = [0_u64; 2];
    for seed in 1..=320 {
        let mut a = generated(seed);
        let mut b = generated(seed);
        let mut naive_bridge = Bridge::default();
        naive_bridge.ops = std::mem::take(&mut a.ops);
        let mut incremental_bridge = Bridge::default();
        incremental_bridge.ops = std::mem::take(&mut b.ops);
        let naive_store = SimpleDataStore::new();
        let incremental_store = SimpleDataStore::new();
        put(&naive_store, "out", Some(Value::F64(0.0)));
        put(&incremental_store, "out", Some(Value::F64(0.0)));
        let naive = lower(a.nodes, &mut naive_bridge, &naive_store, false);
        let incremental = lower(b.nodes, &mut incremental_bridge, &incremental_store, true);
        let mut script = Random(seed ^ 0x9e3779b97f4a7c15);
        let mut last_opted = HashMap::new();
        for step in 0..256 {
            for name in ["a", "b", "c", "d", "out"] {
                if script.next(4) == 0 {
                    let value = (script.next(8) != 0).then(|| Value::F64(script.next(5) as f64));
                    put(&naive_store, name, value.clone());
                    put(&incremental_store, name, value);
                }
            }
            let expected = tick(&naive, &mut naive_bridge);
            assert_eq!(
                tick(&incremental, &mut incremental_bridge),
                expected,
                "seed {seed}, tick {step}"
            );
            assert_eq!(
                cells(&naive),
                cells(&incremental),
                "cells: seed {seed}, tick {step}"
            );
            for (at, bridge) in [&naive_bridge, &incremental_bridge].into_iter().enumerate() {
                opted_calls[at] += bridge
                    .calls
                    .iter()
                    .filter(|(id, _)| a.opted.contains(id))
                    .count() as u64;
                visits[at] += bridge.visits;
            }
            for name in ["a", "b", "c", "d", "out"] {
                assert_eq!(
                    naive_store.slot(&Key::from(name)).get(),
                    incremental_store.slot(&Key::from(name)).get(),
                    "store {name}, seed {seed}, tick {step}"
                );
            }
            let ordinary = |bridge: &Bridge| {
                bridge
                    .calls
                    .iter()
                    .filter(|(id, _)| !a.opted.contains(id))
                    .cloned()
                    .collect::<Vec<_>>()
            };
            assert_eq!(
                ordinary(&naive_bridge),
                ordinary(&incremental_bridge),
                "calls: seed {seed}, tick {step}"
            );
            let mut actual = incremental_bridge
                .calls
                .iter()
                .filter(|(id, _)| a.opted.contains(id))
                .peekable();
            for (id, args) in naive_bridge
                .calls
                .iter()
                .filter(|(id, _)| a.opted.contains(id))
            {
                if actual
                    .peek()
                    .is_some_and(|call| *call == &(*id, args.clone()))
                {
                    actual.next();
                    last_opted.insert(*id, args.clone());
                } else {
                    assert_eq!(
                        last_opted.get(id),
                        Some(args),
                        "skipped changed arguments: seed {seed}, tick {step}, function {id}"
                    );
                }
            }
            assert!(
                actual.next().is_none(),
                "extra opt-in calls: seed {seed}, tick {step}"
            );
        }
    }
    assert!(
        opted_calls[1] < opted_calls[0],
        "caching must reduce _on_change calls: naive {}, incremental {}",
        opted_calls[0],
        opted_calls[1]
    );
    assert!(
        visits[1] < visits[0],
        "caching must reduce node visits: naive {}, incremental {}",
        visits[0],
        visits[1]
    );
}

#[test]
fn an_on_change_leaf_with_a_nested_call_is_never_cached() {
    use crate::schema::CallExpression;
    let mut bridge = Bridge::default();
    bridge.ops.insert(id(20001), Op::Add);
    bridge.ops.insert(id(40000), Op::Mul);
    let input = Expression::Call(Box::new(CallExpression {
        module: Expression::Uuid(fixture::MODULE),
        function: Expression::Uuid(id(40000)),
        arguments: vec![
            (Expression::Uuid(X), lit(2.0)),
            (Expression::Uuid(Y), lit(3.0)),
        ],
    }));
    let mut leaf = module(1, input, true);
    leaf.arguments.insert(_RET_PARAM_ID, variable(8001));
    let tree = lower(vec![leaf], &mut bridge, &SimpleDataStore::new(), true);
    for _ in 0..4 {
        tick(&tree, &mut bridge);
        assert_eq!(
            bridge.calls.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            vec![id(40000), id(20001)]
        );
    }
}

#[test]
fn a_scatter_write_dirties_a_later_readers_cached_ancestor() {
    let store = SimpleDataStore::new();
    let mut bridge = Bridge::default();
    let tree = lower(
        vec![
            control(1, PARALLEL_FUNCTION_ID, vec![2, 3]),
            node(
                2,
                SCATTER_FUNCTION_ID,
                vec![
                    (
                        SCATTER_KEYS_PARAM_ID,
                        Expression::Value(Value::ArrayString(vec!["a".into()])),
                    ),
                    (SCATTER_VALUES_PARAM_ID, variable(9001)),
                    (
                        SCATTER_WITHIN_PARAM_ID,
                        Expression::Value(Value::String("a".into())),
                    ),
                ],
            ),
            control(3, SEQ_FUNCTION_ID, vec![4]),
            node(
                4,
                EQUAL_FUNCTION_ID,
                vec![
                    (EQUAL_A_PARAM_ID, variable(9000)),
                    (EQUAL_B_PARAM_ID, lit(1.0)),
                ],
            ),
        ],
        &mut bridge,
        &store,
        true,
    );
    put(&store, "b", Some(Value::ArrayF64(vec![1.0])));
    assert_eq!(tick(&tree, &mut bridge), Status::Success);
    // The first live read observes Scatter's initial raw-slot write while its
    // ancestor is visiting, leaving that ancestor dirty for one more pass.
    tick(&tree, &mut bridge);
    tick(&tree, &mut bridge);
    assert_eq!(bridge.visits, 2, "only root and effectful scatter visit");
    put(&store, "b", Some(Value::ArrayF64(vec![2.0])));
    assert_eq!(tick(&tree, &mut bridge), Status::Failure);
    assert_eq!(
        bridge.visits, 4,
        "the raw slot write invalidated the nested reader"
    );
}

#[test]
fn changing_the_executor_flag_invalidates_cached_statuses() {
    let mut bridge = Bridge::default();
    let tree = lower(
        vec![node(1, SUCCEED_FUNCTION_ID, vec![])],
        &mut bridge,
        &SimpleDataStore::new(),
        true,
    );
    tick(&tree, &mut bridge);
    tick(&tree, &mut bridge);
    assert_eq!(bridge.visits, 0);
    tree.set_incremental(false);
    tick(&tree, &mut bridge);
    assert_eq!(bridge.visits, 1);
    tree.set_incremental(true);
    tick(&tree, &mut bridge);
    assert_eq!(bridge.visits, 1);
    tick(&tree, &mut bridge);
    assert_eq!(bridge.visits, 0);
}

#[test]
fn unvisited_dirty_children_do_not_prevent_a_terminal_root_from_settling() {
    let mut bridge = Bridge::default();
    bridge.ops.insert(id(20004), Op::Effect);
    bridge.ops.insert(id(20006), Op::Add);
    let mut effect = module(4, lit(0.0), false);
    effect.arguments.insert(STATE, lit(0.0));
    let tree = lower(
        vec![
            control(1, SEQ_FUNCTION_ID, vec![2, 3, 4, 5]),
            node(2, FAIL_FUNCTION_ID, vec![]),
            node(3, RUN_FUNCTION_ID, vec![]),
            effect,
            node(
                5,
                EQUAL_FUNCTION_ID,
                vec![
                    (EQUAL_A_PARAM_ID, port(6, _RET_PARAM_ID)),
                    (EQUAL_B_PARAM_ID, lit(2.0)),
                ],
            ),
            module(6, variable(9000), false),
        ],
        &mut bridge,
        &SimpleDataStore::new(),
        true,
    );
    assert_eq!(tick(&tree, &mut bridge), Status::Failure);
    assert_eq!(bridge.visits, 2);
    assert_eq!(tick(&tree, &mut bridge), Status::Failure);
    assert_eq!(bridge.visits, 0);
    assert!(bridge.calls.is_empty());
}

// Targeted regressions compare statuses, every lowered cell, store keys, and
// ordered ordinary module calls at each tick, before proceeding to the next.
struct Lockstep {
    naive: LoweredTree,
    incremental: LoweredTree,
    naive_bridge: Bridge,
    incremental_bridge: Bridge,
    stores: [SimpleDataStore; 2],
    opted: HashSet<Uuid>,
}
impl Lockstep {
    fn new(nodes: Vec<Node>, ops: HashMap<Uuid, Op>) -> Self {
        let opted = nodes
            .iter()
            .filter(|node| {
                matches!(
                    node.arguments.get(&_ON_CHANGE_PARAM_ID),
                    Some(Expression::Value(Value::Boolean(true)))
                )
            })
            .map(|node| node.function)
            .collect();
        let copy = nodes
            .iter()
            .map(|node| Node {
                id: node.id,
                function: node.function,
                arguments: node.arguments.clone(),
                children: node.children.clone(),
            })
            .collect();
        let stores = [SimpleDataStore::new(), SimpleDataStore::new()];
        let mut naive_bridge = Bridge::default();
        naive_bridge.ops = ops.clone();
        let mut incremental_bridge = Bridge::default();
        incremental_bridge.ops = ops;
        let naive = lower(nodes, &mut naive_bridge, &stores[0], false);
        let incremental = lower(copy, &mut incremental_bridge, &stores[1], true);
        Self {
            naive,
            incremental,
            naive_bridge,
            incremental_bridge,
            stores,
            opted,
        }
    }
    fn put(&self, name: &str, value: Option<Value>) {
        for store in &self.stores {
            put(store, name, value.clone());
        }
    }
    fn tick(&mut self) -> Status {
        let expected = tick(&self.naive, &mut self.naive_bridge);
        assert_eq!(
            tick(&self.incremental, &mut self.incremental_bridge),
            expected
        );
        assert_eq!(cells(&self.incremental), cells(&self.naive));
        for key in ["a", "b", "c", "d", "out", "y", "seen"] {
            assert_eq!(
                self.stores[0].slot(&Key::from(key)).get(),
                self.stores[1].slot(&Key::from(key)).get(),
                "{key}"
            );
        }
        let ordinary = |bridge: &Bridge| {
            bridge
                .calls
                .iter()
                .filter(|(id, _)| !self.opted.contains(id))
                .cloned()
                .collect::<Vec<_>>()
        };
        assert_eq!(
            ordinary(&self.incremental_bridge),
            ordinary(&self.naive_bridge)
        );
        expected
    }
}

/// Unlike a later sibling's write, a module composite's raw write must reach
/// its child before the composite returns and before the child's cache answers.
#[test]
fn a_module_composites_raw_store_write_reaches_its_cached_child() {
    let composite = id(20001);
    let mut pair = Lockstep::new(
        vec![
            control(1, composite, vec![2]),
            node(
                2,
                EQUAL_FUNCTION_ID,
                vec![
                    (EQUAL_A_PARAM_ID, variable(9000)),
                    (EQUAL_B_PARAM_ID, lit(0.0)),
                ],
            ),
        ],
        HashMap::from([(composite, Op::WriteThenTick)]),
    );
    pair.naive_bridge.raw_write = Some(pair.stores[0].slot(&Key::from("a")));
    pair.incremental_bridge.raw_write = Some(pair.stores[1].slot(&Key::from("a")));
    // Tick 1 leaves the boundary value unchanged, so the child's cached
    // Success is clean when the composite writes 1 during tick 2.
    pair.put("a", Some(Value::F64(0.0)));
    for (step, expected) in [Status::Success, Status::Failure, Status::Failure]
        .into_iter()
        .enumerate()
    {
        assert_eq!(pair.tick(), expected);
        assert_eq!(
            pair.stores[1].slot(&Key::from("a")).get(),
            Some(Value::F64(step as f64))
        );
    }
}

#[test]
fn own_argument_versions_keep_feedback_writers_dirty() {
    for input in [8001, 9000] {
        let mut leaf = module(1, variable(input), true);
        leaf.arguments.insert(_RET_PARAM_ID, variable(input));
        let mut pair = Lockstep::new(vec![leaf], HashMap::from([(id(20001), Op::Add)]));
        for n in 1..=4 {
            assert_eq!(pair.tick(), Status::Success);
            assert_eq!(
                pair.incremental.tree().variables.borrow()[&id(input)].get(),
                Some(Value::F64(2.0 * n as f64))
            );
            assert_eq!(pair.incremental_bridge.calls.len(), 1);
        }
    }
}

fn feedback_regex(n: u128) -> Node {
    node(
        n,
        REGEX_MATCH_FUNCTION_ID,
        vec![
            (REGEX_MATCH_VALUE_PARAM_ID, variable(9000)),
            (
                REGEX_MATCH_MATCHER_PARAM_ID,
                Expression::Value(Value::String("\\Ba".into())),
            ),
            (REGEX_MATCH_FIRST_MATCH_PARAM_ID, variable(9000)),
        ],
    )
}

#[test]
fn native_in_out_arguments_dirty_their_own_reader() {
    let mut pair = Lockstep::new(vec![feedback_regex(1)], HashMap::new());
    pair.put("a", Some(Value::String("ba".into())));
    for expected in [Status::Success, Status::Failure, Status::Failure] {
        assert_eq!(pair.tick(), expected);
    }
}

#[test]
fn native_feedback_preserves_short_circuit_effectful_calls() {
    let mut effect = module(3, lit(0.0), false);
    effect.arguments.insert(STATE, lit(0.0));
    let mut pair = Lockstep::new(
        vec![
            control(1, SEQ_FUNCTION_ID, vec![2, 3]),
            feedback_regex(2),
            effect,
        ],
        HashMap::from([(id(20003), Op::Effect)]),
    );
    pair.put("a", Some(Value::String("ba".into())));
    for expected in [Status::Success, Status::Failure, Status::Failure] {
        assert_eq!(pair.tick(), expected);
        assert_eq!(
            pair.incremental_bridge.calls.len(),
            usize::from(expected == Status::Success)
        );
    }
}

fn alias_pair(visited_ancestor: bool) -> Lockstep {
    let mut writer = module(2, variable(9001), true);
    writer.arguments.insert(_RET_PARAM_ID, variable(9010));
    let mut nodes = vec![
        control(1, SEQ_FUNCTION_ID, vec![2, 3]),
        writer,
        control(
            3,
            if visited_ancestor {
                FALLBACK_FUNCTION_ID
            } else {
                SEQ_FUNCTION_ID
            },
            if visited_ancestor {
                vec![5, 4]
            } else {
                vec![4]
            },
        ),
        node(
            4,
            EQUAL_FUNCTION_ID,
            vec![
                (EQUAL_A_PARAM_ID, variable(9000)),
                (EQUAL_B_PARAM_ID, lit(3.0)),
            ],
        ),
    ];
    if visited_ancestor {
        nodes.push(node(
            5,
            EQUAL_FUNCTION_ID,
            vec![
                (EQUAL_A_PARAM_ID, variable(9001)),
                (EQUAL_B_PARAM_ID, lit(99.0)),
            ],
        ));
    }
    Lockstep::new(nodes, HashMap::from([(id(20002), Op::Add)]))
}

#[test]
fn lazy_store_observation_reaches_aliases_under_a_clean_ancestor() {
    let mut pair = alias_pair(false);
    assert_eq!(pair.tick(), Status::Failure);
    assert_eq!(pair.tick(), Status::Failure);
    pair.put("b", Some(Value::F64(1.0)));
    assert_eq!(pair.tick(), Status::Success);
    assert_eq!(pair.tick(), Status::Success);
}

#[test]
fn lazy_store_observation_checks_a_clean_child_of_a_dirty_ancestor() {
    let mut pair = alias_pair(true);
    assert_eq!(pair.tick(), Status::Failure);
    assert_eq!(pair.tick(), Status::Failure);
    pair.put("b", Some(Value::F64(1.0)));
    assert_eq!(pair.tick(), Status::Success);
    assert_eq!(pair.tick(), Status::Success);
}

#[test]
fn only_running_nodes_visited_last_tick_keep_polling_alive() {
    let mut pair = Lockstep::new(
        vec![
            control(1, FALLBACK_FUNCTION_ID, vec![2, 3]),
            node(
                2,
                EQUAL_FUNCTION_ID,
                vec![
                    (EQUAL_A_PARAM_ID, variable(9000)),
                    (EQUAL_B_PARAM_ID, lit(1.0)),
                ],
            ),
            node(3, RUN_FUNCTION_ID, vec![]),
        ],
        HashMap::new(),
    );
    pair.put("a", Some(Value::F64(0.0)));
    assert_eq!(pair.tick(), Status::Running);
    pair.put("a", Some(Value::F64(1.0)));
    assert_eq!(pair.tick(), Status::Success);
    for _ in 0..5 {
        assert_eq!(pair.tick(), Status::Success);
        assert_eq!(pair.incremental_bridge.visits, 0);
    }
    pair.put("a", Some(Value::F64(0.0)));
    assert_eq!(pair.tick(), Status::Running);
    assert_eq!(
        pair.incremental_bridge.visits, 3,
        "cached Running cannot answer"
    );
}

#[test]
fn detached_return_fed_into_a_store_named_input_is_pulled_before_skipping() {
    let mut detached = module(3, variable(9000), false);
    detached
        .arguments
        .insert(_RET_PARAM_ID, port(2, EQUAL_A_PARAM_ID));
    let mut pair = Lockstep::new(
        vec![
            control(1, SEQ_FUNCTION_ID, vec![2]),
            node(
                2,
                EQUAL_FUNCTION_ID,
                vec![
                    (EQUAL_A_PARAM_ID, variable(9001)),
                    (EQUAL_B_PARAM_ID, lit(3.0)),
                ],
            ),
            detached,
        ],
        HashMap::from([(id(20003), Op::Add)]),
    );
    pair.put("a", Some(Value::F64(0.0)));
    assert_eq!(pair.tick(), Status::Failure);
    assert_eq!(pair.tick(), Status::Failure);
    pair.put("a", Some(Value::F64(1.0)));
    assert_eq!(pair.tick(), Status::Success);
    assert_eq!(pair.tick(), Status::Success);
}

#[test]
fn stateful_on_change_computation_runs_until_its_state_settles() {
    let mut leaf = module(1, lit(8.0), true);
    leaf.arguments.insert(STATE, lit(0.0));
    leaf.arguments.insert(_RET_PARAM_ID, variable(8001));
    let mut pair = Lockstep::new(vec![leaf], HashMap::from([(id(20001), Op::Approach)]));
    for _ in 0..15 {
        pair.tick();
        assert_eq!(pair.incremental_bridge.calls.len(), 1);
    }
    for _ in 0..5 {
        pair.tick();
        assert!(pair.incremental_bridge.calls.is_empty());
        assert_eq!(pair.incremental_bridge.visits, 0);
    }
}

/// Attached and detached nodes accept and display the reserved argument even
/// though the module signature has no such parameter, and never pass it along.
#[test]
fn reserved_on_change_argument_never_crosses_the_module_bridge() {
    for detached in [false, true] {
        let mut leaf = module(1, lit(0.0), true);
        let index = HashMap::from([fixture::signature(leaf.function)]);
        assert!(leaf
            .display(crate::display::Names::functions(&index))
            .to_string()
            .contains("_on_change: true"));
        leaf.arguments.insert(_RET_PARAM_ID, variable(8001));
        let mut nodes = Vec::new();
        if detached {
            nodes.push(node(
                2,
                EQUAL_FUNCTION_ID,
                vec![
                    (EQUAL_A_PARAM_ID, port(1, _RET_PARAM_ID)),
                    (EQUAL_B_PARAM_ID, lit(2.0)),
                ],
            ));
        }
        nodes.push(leaf);
        let mut pair = Lockstep::new(nodes, HashMap::from([(id(20001), Op::Add)]));
        pair.tick();
        for bridge in [&pair.naive_bridge, &pair.incremental_bridge] {
            assert_eq!(bridge.calls.len(), 1);
            assert!(bridge
                .calls
                .iter()
                .all(|(_, args)| args.iter().all(|field| field.id != _ON_CHANGE_PARAM_ID)));
        }
    }
}

#[test]
fn only_literal_true_opts_a_module_leaf_into_caching() {
    for argument in [
        Expression::Value(Value::Boolean(false)),
        lit(1.0),
        variable(9000),
    ] {
        let mut leaf = module(1, lit(0.0), false);
        leaf.arguments.insert(_ON_CHANGE_PARAM_ID, argument);
        leaf.arguments.insert(_RET_PARAM_ID, variable(8001));
        let mut pair = Lockstep::new(vec![leaf], HashMap::from([(id(20001), Op::Add)]));
        pair.put("a", Some(Value::Boolean(true)));
        for _ in 0..4 {
            pair.tick();
            assert_eq!(pair.incremental_bridge.calls.len(), 1);
        }
    }
}

#[test]
fn effectful_native_leaves_never_cache_terminal_results() {
    use arora_types::call::Call;
    for leaf in [
        node(
            1,
            SET_STR_FUNCTION_ID,
            vec![
                (SET_STR_VARIABLE_PARAM_ID, variable(9000)),
                (
                    SET_STR_VALUE_PARAM_ID,
                    Expression::Value(Value::String("hello".into())),
                ),
            ],
        ),
        node(
            1,
            UNSET_STR_FUNCTION_ID,
            vec![(UNSET_STR_VARIABLE_PARAM_ID, variable(9000))],
        ),
        node(
            1,
            RUN_CALL_FUNCTION_ID,
            vec![(
                RUN_CALL_PARAM_ID,
                Expression::Value(
                    arora_types::value_serde::to_value(&Call {
                        module_id: Some(fixture::MODULE),
                        id: id(20002),
                        args: vec![],
                    })
                    .unwrap(),
                ),
            )],
        ),
    ] {
        let mut pair = Lockstep::new(vec![leaf], HashMap::from([(id(20002), Op::Add)]));
        for _ in 0..4 {
            pair.tick();
            assert_eq!(pair.incremental_bridge.visits, 1);
        }
    }
}

#[test]
fn a_call_nested_under_select_disables_caching() {
    use crate::schema::CallExpression;
    let input = Expression::Select {
        source: Box::new(Expression::Call(Box::new(CallExpression {
            module: Expression::Uuid(fixture::MODULE),
            function: Expression::Uuid(id(40000)),
            arguments: vec![
                (Expression::Uuid(X), lit(2.0)),
                (Expression::Uuid(Y), lit(3.0)),
            ],
        }))),
        path: Key::new(""),
    };
    let mut leaf = module(1, input, true);
    leaf.arguments.insert(_RET_PARAM_ID, variable(8001));
    let mut pair = Lockstep::new(
        vec![leaf],
        HashMap::from([(id(20001), Op::Add), (id(40000), Op::Mul)]),
    );
    for _ in 0..4 {
        pair.tick();
        assert_eq!(
            pair.incremental_bridge
                .calls
                .iter()
                .map(|(id, _)| *id)
                .collect::<Vec<_>>(),
            vec![id(40000), id(20001)]
        );
    }
}

#[test]
fn cells_without_subscribers_do_not_observe_writes() {
    let store = SimpleDataStore::new();
    let mut bridge = Bridge::default();
    let tree = lower(
        vec![node(
            1,
            EQUAL_FUNCTION_ID,
            vec![
                (EQUAL_A_PARAM_ID, variable(9000)),
                (EQUAL_B_PARAM_ID, lit(0.0)),
            ],
        )],
        &mut bridge,
        &store,
        false,
    );
    let cell = tree.tree().variables.borrow()[&id(9000)].clone();
    cell.set(Value::F64(1.0));
    assert_eq!(cell.version(), 0);
    tree.set_incremental(true);
    cell.set(Value::F64(1.0));
    assert_eq!(cell.version(), 0, "seen is initialized when subscribing");
    cell.set(Value::F64(2.0));
    assert_eq!(cell.version(), 1);
    tree.set_incremental(false);
    cell.set(Value::F64(3.0));
    assert_eq!(cell.version(), 1, "disabling drops subscriptions");
    tree.set_incremental(true);
    cell.set(Value::F64(3.0));
    assert_eq!(cell.version(), 1);
}
