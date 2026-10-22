//! `Scatter`: what it writes, when it fails or errors writing nothing, how its
//! `within` is bound, and the slots it keeps across ticks.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use arora_behavior::graph::{Graph, GraphDiff, Io, Link, LinkSource, Node as GraphNode, Port};
use arora_behavior::{BehaviorContext, BehaviorInterpreter, FunctionIndex, Status};
use arora_simple_data_store::SimpleDataStore;
use arora_types::call::{Call, CallBridge, CallError, CallResult, Callable, CallableId};
use arora_types::data::{DataStore, Key, Slot, StateChange};
use arora_types::gen_uuid_from_str;
use arora_types::value::Value;
use uuid::Uuid;

use crate::behavior::BehaviorTreeInterpreter;
use crate::error::BehaviorTreeError;
use crate::graph::build_behavior_tree;
use crate::lower_behavior_tree;
use crate::nodes::{
    SCATTER_FUNCTION_ID, SCATTER_KEYS_PARAM_ID, SCATTER_VALUES_PARAM_ID, SCATTER_WITHIN_PARAM_ID,
    SEQ_FUNCTION_ID, UNPACK_FUNCTION_ID, UNPACK_OUT_PARAM_IDS, UNPACK_VALUES_PARAM_ID,
};
use crate::schema::Expression;
use crate::variable::VariableResolver;

const SEQ: Uuid = Uuid::from_u128(0x200);
const SCATTER: Uuid = Uuid::from_u128(0x201);
const MARK: Uuid = Uuid::from_u128(0x202);

/// Registers the tree's tick functions and calls them back; the trees here
/// call no module.
#[derive(Default)]
struct Bridge {
    registered: HashMap<u64, Rc<dyn Callable>>,
    next_id: u64,
}

impl CallBridge for Bridge {
    fn arora_call(&mut self, call: Call) -> Result<CallResult, CallError> {
        Err(CallError::FunctionNotFound { id: call.id })
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
    fn arora_call_indirect(&mut self, id: &CallableId) -> Result<Value, CallError> {
        let callable = self.registered[&id.id].clone();
        callable.call(self)
    }
}

/// The variable named `name`.
fn variable(name: &str) -> Uuid {
    gen_uuid_from_str(name)
}

/// A link source reading the variable `name`: under the Direct convention,
/// the store key of that name, or a tree-local cell for a `.` name.
fn named(name: &str) -> LinkSource {
    LinkSource::Variable(variable(name))
}

fn literal(value: Value) -> LinkSource {
    LinkSource::Literal(value)
}

fn strings(names: &[&str]) -> Value {
    Value::ArrayString(names.iter().map(|name| name.to_string()).collect())
}

fn floats(values: &[f64]) -> Value {
    Value::ArrayF64(values.to_vec())
}

fn graph_node(id: Uuid, function: Uuid, inputs: &[Uuid]) -> GraphNode {
    GraphNode {
        id,
        function,
        inputs: inputs.iter().copied().map(Io::new).collect(),
        ..GraphNode::default()
    }
}

/// `Sequence(Scatter(values, keys, within), Unpack([true], out_0 → {done}))`:
/// `done` is written when the `Scatter` succeeded. A `None` argument is left
/// unlinked. Every variable a link reads is named after `named`'s convention.
fn scatter_graph(
    values: Option<LinkSource>,
    keys: Option<LinkSource>,
    within: Option<LinkSource>,
) -> Graph {
    let mut graph = Graph::empty();
    graph.root = Some(SEQ);
    graph.nodes.insert(
        SEQ,
        GraphNode {
            children: Some(vec![SCATTER, MARK]),
            ..graph_node(SEQ, SEQ_FUNCTION_ID, &[])
        },
    );
    graph.nodes.insert(
        SCATTER,
        graph_node(
            SCATTER,
            SCATTER_FUNCTION_ID,
            &[
                SCATTER_VALUES_PARAM_ID,
                SCATTER_KEYS_PARAM_ID,
                SCATTER_WITHIN_PARAM_ID,
            ],
        ),
    );
    graph.nodes.insert(
        MARK,
        graph_node(MARK, UNPACK_FUNCTION_ID, &[UNPACK_VALUES_PARAM_ID]),
    );
    let link = |node, port, source| Link::new(Port::new(node, port), source);
    for (port, source) in [
        (SCATTER_VALUES_PARAM_ID, values),
        (SCATTER_KEYS_PARAM_ID, keys),
        (SCATTER_WITHIN_PARAM_ID, within),
    ] {
        if let Some(source) = source {
            graph.links.push(link(SCATTER, port, source));
        }
    }
    graph.links.push(link(
        MARK,
        UNPACK_VALUES_PARAM_ID,
        literal(Value::ArrayBoolean(vec![true])),
    ));
    graph
        .links
        .push(link(MARK, UNPACK_OUT_PARAM_IDS[0], named("done")));
    for name in ["done", "table/values", "table/keys", "prefix", ".keys"] {
        graph.variables.insert(variable(name), name.to_string());
    }
    graph
}

/// The `Scatter` graph with all three arguments linked.
fn scatter(values: LinkSource, keys: LinkSource, within: &str) -> Graph {
    scatter_graph(
        Some(values),
        Some(keys),
        Some(literal(Value::String(within.to_string()))),
    )
}

/// An interpreter running `graph` as its main behavior over a store.
struct Device {
    store: SimpleDataStore,
    interp: BehaviorTreeInterpreter,
    bridge: Bridge,
}

impl Device {
    fn new(graph: Graph) -> Self {
        let mut interp = BehaviorTreeInterpreter::new();
        interp.load(graph).expect("the graph loads");
        Device {
            store: SimpleDataStore::new(),
            interp,
            bridge: Bridge::default(),
        }
    }

    fn tick(&mut self) -> Result<(), String> {
        let mut ctx = BehaviorContext {
            functions: &FunctionIndex::new(),
            store: &self.store,
            call_bridge: &mut self.bridge,
        };
        self.interp
            .tick(&mut ctx)
            .map(|_| ())
            .map_err(|e| e.message)
    }

    fn read(&self, keys: &[&str]) -> Vec<Option<Value>> {
        let keys: Vec<Key> = keys.iter().map(|key| Key::new(*key)).collect();
        self.store.read(&keys)
    }

    fn set(&self, key: &str, value: Value) {
        self.store
            .write(StateChange {
                set: HashMap::from([(Key::new(key), Some(value))]),
                unset: Default::default(),
            })
            .expect("the store writes");
    }

    /// Whether the `Scatter` succeeded since the last call.
    fn succeeded(&self) -> bool {
        let done = self.read(&["done"])[0].is_some();
        self.store
            .write(StateChange {
                set: HashMap::new(),
                unset: [Key::new("done")].into(),
            })
            .expect("the store writes");
        done
    }
}

#[test]
fn writes_each_value_to_the_key_its_index_names() {
    let mut device = Device::new(scatter(
        literal(floats(&[1.0, 2.0, 3.0])),
        literal(strings(&["anim/a", "anim/b/c", "anim"])),
        "anim",
    ));
    device.tick().unwrap();
    assert!(device.succeeded());
    assert_eq!(
        device.read(&["anim/a", "anim/b/c", "anim"]),
        vec![
            Some(Value::F64(1.0)),
            Some(Value::F64(2.0)),
            Some(Value::F64(3.0))
        ],
        "a key may be `within` itself"
    );
}

#[test]
fn keys_may_be_an_array_of_string_values() {
    let mut device = Device::new(scatter(
        literal(Value::ArrayValue(vec![Value::U8(1), Value::Boolean(true)])),
        literal(Value::ArrayValue(vec![
            Value::String("anim/a".to_string()),
            Value::String("anim/b".to_string()),
        ])),
        "anim",
    ));
    device.tick().unwrap();
    assert!(device.succeeded());
    assert_eq!(
        device.read(&["anim/a", "anim/b"]),
        vec![Some(Value::U8(1)), Some(Value::Boolean(true))]
    );
}

#[test]
fn a_unit_element_leaves_its_key_as_it_is() {
    let mut device = Device::new(scatter(
        literal(Value::ArrayValue(vec![Value::F64(1.0), Value::Unit])),
        literal(strings(&["anim/a", "anim/b"])),
        "anim",
    ));
    device.set("anim/b", Value::F64(9.0));
    device.tick().unwrap();
    assert!(device.succeeded());
    assert_eq!(
        device.read(&["anim/a", "anim/b"]),
        vec![Some(Value::F64(1.0)), Some(Value::F64(9.0))]
    );
}

#[test]
fn fails_writing_nothing_when_the_lengths_differ() {
    for (values, keys) in [
        (floats(&[1.0, 2.0]), strings(&["anim/a"])),
        (floats(&[1.0]), strings(&["anim/a", "anim/b"])),
    ] {
        let mut device = Device::new(scatter(literal(values), literal(keys), "anim"));
        device.tick().unwrap();
        assert!(!device.succeeded());
        assert_eq!(device.read(&["anim/a", "anim/b"]), vec![None, None]);
    }

    // With the keys the previous tick resolved, too.
    let mut device = Device::new(scatter(
        named("table/values"),
        literal(strings(&["anim/a", "anim/b"])),
        "anim",
    ));
    device.set("table/values", floats(&[1.0, 2.0]));
    device.tick().unwrap();
    assert!(device.succeeded());
    device.set("table/values", floats(&[3.0]));
    device.tick().unwrap();
    assert!(!device.succeeded());
    assert_eq!(
        device.read(&["anim/a", "anim/b"]),
        vec![Some(Value::F64(1.0)), Some(Value::F64(2.0))]
    );
}

#[test]
fn fails_writing_nothing_on_values_or_keys_that_are_not_arrays_of_their_kind() {
    for (values, keys) in [
        (Value::F64(1.0), strings(&["anim/a"])),
        (floats(&[1.0]), Value::String("anim/a".to_string())),
        (floats(&[1.0]), floats(&[1.0])),
        (floats(&[1.0]), Value::ArrayValue(vec![Value::U8(1)])),
        // Not arrays of their kind, whatever the other's length.
        (Value::F64(1.0), strings(&[])),
        (floats(&[]), Value::String("anim/a".to_string())),
        (floats(&[]), Value::ArrayValue(vec![Value::U8(1)])),
    ] {
        let mut device = Device::new(scatter(
            literal(values.clone()),
            literal(keys.clone()),
            "anim",
        ));
        device.tick().unwrap();
        assert!(!device.succeeded(), "{values} to {keys}");
        assert_eq!(device.read(&["anim/a"]), vec![None]);
    }
}

#[test]
fn a_key_outside_within_errors_the_tick_and_writes_nothing() {
    for outside in ["other/b", "animation/b", "an", ".anim/b"] {
        let mut device = Device::new(scatter(
            literal(floats(&[1.0, 2.0])),
            literal(strings(&["anim/a", outside])),
            "anim",
        ));
        let error = device.tick().expect_err(outside);
        assert!(error.contains(outside), "{error}");
        assert_eq!(device.read(&["anim/a", outside]), vec![None, None]);
        assert!(!device.succeeded());
    }
}

/// An empty `within`, or none, is the tree's whole namespace: keys at its root
/// and under paths that share no prefix are written alike.
#[test]
fn an_empty_or_omitted_within_writes_anywhere_in_the_namespace() {
    let keys = [
        "root",
        "hip_yaw/target.position",
        "eye.position",
        "anim/face/k0",
    ];
    for within in [Some(""), None] {
        let mut device = Device::new(scatter_graph(
            Some(literal(floats(&[1.0, 2.0, 3.0, 4.0]))),
            Some(literal(strings(&keys))),
            within.map(|text| literal(Value::String(text.to_string()))),
        ));
        device.tick().unwrap();
        assert!(device.succeeded(), "within {within:?}");
        assert_eq!(
            device.read(&keys),
            vec![
                Some(Value::F64(1.0)),
                Some(Value::F64(2.0)),
                Some(Value::F64(3.0)),
                Some(Value::F64(4.0))
            ],
            "within {within:?}"
        );
    }
}

/// A tree-local name is no store key: whatever `within` is, a key that is one
/// errors the tick as a key outside `within` does, an inconsistent tree, and
/// nothing is written.
#[test]
fn a_tree_local_key_errors_the_tick_and_writes_nothing() {
    for within in [Some("anim"), Some(""), None] {
        let counted = Counted::new();
        let graph = scatter_graph(
            Some(literal(floats(&[1.0, 2.0]))),
            Some(literal(strings(&["anim/a", ".anim/b"]))),
            within.map(|text| literal(Value::String(text.to_string()))),
        );
        let resolver = counted.resolver("");
        let tree = build_behavior_tree(&graph, &*resolver)
            .unwrap()
            .with_key_resolver(resolver);
        let mut bridge = Bridge::default();
        let lowered = lower_behavior_tree(tree, &FunctionIndex::new(), &mut bridge, false).unwrap();
        let error = format!("{}", lowered.tick(&mut bridge).expect_err("tree-local"));
        let refusal = BehaviorTreeError::InconsistentTreeError {
            message: format!("Scatter node {SCATTER}: the key '.anim/b'"),
        };
        assert!(
            error.starts_with(&refusal.to_string()),
            "within {within:?}: {error}"
        );
        assert_eq!(
            counted
                .store
                .read(&[Key::new("anim/a"), Key::new(".anim/b")]),
            vec![None, None],
            "within {within:?}"
        );
    }
}

#[test]
fn a_key_named_twice_errors_the_tick_and_writes_nothing() {
    let mut device = Device::new(scatter(
        literal(floats(&[1.0, 2.0, 3.0])),
        literal(strings(&["anim/a", "anim/b", "anim/a"])),
        "anim",
    ));
    let error = device.tick().expect_err("a key named twice");
    assert!(error.contains("anim/a"), "{error}");
    assert_eq!(device.read(&["anim/a", "anim/b"]), vec![None, None]);
}

#[test]
fn within_is_a_literal_key_prefix_or_none_and_values_and_keys_are_linked() {
    let lowers = |graph: Graph| build_behavior_tree(&graph, &|_| None).map(|_| ());
    let values = || Some(literal(floats(&[1.0])));
    let keys = || Some(literal(strings(&["anim/a"])));
    let within = |text: &str| Some(literal(Value::String(text.to_string())));
    lowers(scatter_graph(values(), keys(), within("anim"))).expect("a key prefix");
    lowers(scatter_graph(values(), keys(), within("anim/face"))).expect("a deeper prefix");
    lowers(scatter_graph(values(), keys(), within(""))).expect("the whole namespace");
    lowers(scatter_graph(values(), keys(), None)).expect("`within` left out");
    for text in ["anim/", ".anim", "/", "."] {
        assert!(
            lowers(scatter_graph(values(), keys(), within(text))).is_err(),
            "'{text}' is not a key prefix"
        );
    }
    assert!(
        lowers(scatter_graph(values(), keys(), Some(named("prefix")))).is_err(),
        "`within` read from a variable"
    );
    assert!(
        lowers(scatter_graph(values(), keys(), Some(literal(Value::U8(1))))).is_err(),
        "`within` that is not a string"
    );
    for (graph, missing) in [
        (scatter_graph(None, keys(), within("anim")), "values"),
        (scatter_graph(values(), None, within("anim")), "keys"),
    ] {
        assert!(lowers(graph).is_err(), "{missing} is not linked");
    }

    // An edit that would leave `within` a variable is refused.
    let mut interp = BehaviorTreeInterpreter::new();
    interp
        .load(scatter_graph(values(), keys(), within("anim")))
        .unwrap();
    assert!(interp
        .apply(GraphDiff {
            add_links: vec![Link::new(
                Port::new(SCATTER, SCATTER_WITHIN_PARAM_ID),
                named("prefix"),
            )],
            ..GraphDiff::default()
        })
        .is_err());
}

/// The store notifies changes only, so a value written again makes none.
#[test]
fn a_value_that_does_not_move_makes_no_store_change() {
    let mut device = Device::new(scatter(
        named("table/values"),
        literal(strings(&["anim/a", "anim/b"])),
        "anim",
    ));
    let changes = device.store.subscribe_prefix("anim");
    let drain = || {
        let mut counts: HashMap<String, usize> = HashMap::new();
        while let Some(change) = changes.try_recv() {
            for key in change.set.keys() {
                *counts.entry(key.path.clone()).or_default() += 1;
            }
        }
        counts
    };
    drain();
    device.set("table/values", floats(&[1.0, 2.0]));
    device.tick().unwrap();
    device.tick().unwrap();
    let counts = drain();
    assert_eq!(counts.get("anim/a"), Some(&1), "{counts:?}");
    assert_eq!(counts.get("anim/b"), Some(&1), "{counts:?}");
    device.set("table/values", floats(&[1.0, 3.0]));
    device.tick().unwrap();
    let counts = drain();
    assert_eq!(counts.get("anim/a"), None, "the held value makes no change");
    assert_eq!(counts.get("anim/b"), Some(&1), "the moved value changes");
}

/// Keys and values read from the store: a tick with other keys writes to
/// them, and leaves the keys it no longer names as they are.
#[test]
fn other_keys_rebind_the_node_to_their_slots() {
    let mut device = Device::new(scatter(named("table/values"), named("table/keys"), "anim"));
    device.set("table/keys", strings(&["anim/a", "anim/b"]));
    device.set("table/values", floats(&[1.0, 2.0]));
    device.tick().unwrap();
    device.set("table/keys", strings(&["anim/b", "anim/c", "anim/d"]));
    device.set("table/values", floats(&[3.0, 4.0, 5.0]));
    device.tick().unwrap();
    assert!(device.succeeded());
    assert_eq!(
        device.read(&["anim/a", "anim/b", "anim/c", "anim/d"]),
        vec![
            Some(Value::F64(1.0)),
            Some(Value::F64(3.0)),
            Some(Value::F64(4.0)),
            Some(Value::F64(5.0))
        ]
    );
}

/// A table of any length is written in one tick: 500 keys, past `Unpack`'s
/// 64 ports.
#[test]
fn writes_a_table_of_500_keys_in_one_tick() {
    let names: Vec<String> = (0..500).map(|i| format!("anim/k{i}")).collect();
    let mut device = Device::new(scatter(
        literal(Value::ArrayF64((0..500).map(f64::from).collect())),
        literal(Value::ArrayString(names.clone())),
        "anim",
    ));
    device.tick().unwrap();
    assert!(device.succeeded());
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    assert_eq!(
        device.read(&names),
        (0..500)
            .map(|i| Some(Value::F64(f64::from(i))))
            .collect::<Vec<_>>()
    );
}

/// A store, and a resolver over it that counts the keys it resolves.
struct Counted {
    store: SimpleDataStore,
    resolved: Rc<Cell<usize>>,
}

impl Counted {
    fn new() -> Self {
        Counted {
            store: SimpleDataStore::new(),
            resolved: Rc::new(Cell::new(0)),
        }
    }

    /// The resolver, which declines `declined`.
    fn resolver(&self, declined: &'static str) -> Rc<VariableResolver<'static>> {
        let (store, resolved) = (self.store.clone(), self.resolved.clone());
        Rc::new(move |name: &str| -> Option<Box<dyn Slot>> {
            resolved.set(resolved.get() + 1);
            (name != declined).then(|| store.slot(&Key::from(name)))
        })
    }
}

/// A tick whose keys equal the previous tick's resolves none of them.
#[test]
fn the_same_keys_resolve_no_slot_again() {
    let counted = Counted::new();
    let keys = RefCell::new(strings(&["anim/a", "anim/b"]));
    let graph = scatter(literal(floats(&[1.0, 2.0])), named(".keys"), "anim");
    let resolver = counted.resolver("");
    let tree = build_behavior_tree(&graph, &*resolver)
        .unwrap()
        .with_key_resolver(resolver);
    let mut bridge = Bridge::default();
    let lowered = lower_behavior_tree(tree, &FunctionIndex::new(), &mut bridge, false).unwrap();
    let cell = lowered.tree().variables.borrow()[&variable(".keys")].clone();
    let mut tick = || {
        cell.set(keys.borrow().clone());
        let resolved = counted.resolved.get();
        assert_eq!(lowered.tick(&mut bridge).unwrap(), Status::Success);
        counted.resolved.get() - resolved
    };
    assert_eq!(tick(), 2, "the first tick resolves its keys");
    assert_eq!(tick(), 0, "the same keys resolve none");
    *keys.borrow_mut() = strings(&["anim/a", "anim/c"]);
    assert_eq!(tick(), 2, "other keys resolve all again");
    assert_eq!(tick(), 0);
    assert_eq!(
        counted.store.read(&[Key::new("anim/c")]),
        vec![Some(Value::F64(2.0))]
    );
}

#[test]
fn a_key_without_a_store_slot_errors_the_tick_and_writes_nothing() {
    let counted = Counted::new();
    let graph = scatter(
        literal(floats(&[1.0, 2.0])),
        literal(strings(&["anim/a", "anim/b"])),
        "anim",
    );
    let resolver = counted.resolver("anim/b");
    let tree = build_behavior_tree(&graph, &*resolver)
        .unwrap()
        .with_key_resolver(resolver);
    let mut bridge = Bridge::default();
    let lowered = lower_behavior_tree(tree, &FunctionIndex::new(), &mut bridge, false).unwrap();
    let error = format!("{}", lowered.tick(&mut bridge).expect_err("no slot"));
    assert!(error.contains("anim/b"), "{error}");
    assert_eq!(counted.store.read(&[Key::new("anim/a")]), vec![None]);
}

/// Lengths that differ fail the node before its keys resolve: neither a key
/// without a slot, a key outside `within`, a key named twice nor a tree
/// without a resolver turns a table that does not match yet into an error of
/// the tick.
#[test]
fn lengths_that_differ_fail_before_the_keys_resolve() {
    for (keys, declined) in [
        (strings(&["anim/a"]), "anim/a"),
        (strings(&["other/a"]), ""),
        (strings(&["anim/a", "anim/a"]), ""),
    ] {
        let counted = Counted::new();
        let graph = scatter(literal(floats(&[])), literal(keys.clone()), "anim");
        let resolver = counted.resolver(declined);
        let tree = build_behavior_tree(&graph, &*resolver)
            .unwrap()
            .with_key_resolver(resolver);
        let mut bridge = Bridge::default();
        let lowered = lower_behavior_tree(tree, &FunctionIndex::new(), &mut bridge, false).unwrap();
        let resolved = counted.resolved.get();
        assert_eq!(
            lowered.tick(&mut bridge).unwrap(),
            Status::Failure,
            "{keys}"
        );
        assert_eq!(counted.resolved.get(), resolved, "{keys}: no key resolved");
    }

    let graph = scatter(literal(floats(&[])), literal(strings(&["anim/a"])), "anim");
    let tree = build_behavior_tree(&graph, &|_| None).unwrap();
    let mut bridge = Bridge::default();
    let lowered = lower_behavior_tree(tree, &FunctionIndex::new(), &mut bridge, false).unwrap();
    assert_eq!(lowered.tick(&mut bridge).unwrap(), Status::Failure);
}

#[test]
fn a_tree_given_no_key_resolver_errors_when_it_scatters() {
    let graph = scatter(
        literal(floats(&[1.0])),
        literal(strings(&["anim/a"])),
        "anim",
    );
    let tree = build_behavior_tree(&graph, &|_| None).unwrap();
    let mut bridge = Bridge::default();
    let lowered = lower_behavior_tree(tree, &FunctionIndex::new(), &mut bridge, false).unwrap();
    let error = format!("{}", lowered.tick(&mut bridge).expect_err("no store"));
    assert!(error.contains("no store"), "{error}");
}

/// `Scatter` reads from Groot with its three ports, exports back to them, and
/// the palette declares them.
#[test]
fn scatter_round_trips_through_groot() {
    use crate::schema_groot::{BehaviorTree, Node};
    let groot = |tree: &str| {
        BehaviorTree::try_from_groot_xml(&format!(
            r#"<root main_tree_to_execute="MainTree"><BehaviorTree ID="MainTree">{tree}</BehaviorTree></root>"#
        ))
        .unwrap()
    };
    let mut names = HashMap::new();
    let node =
        groot(r#"<Scatter values="{.values}" keys='json:{"strs":["anim/a"]}' within="anim"/>"#)
            .root
            .try_into_tree_node(&HashMap::new(), &mut names)
            .expect("the tag resolves");
    assert_eq!(node.function, SCATTER_FUNCTION_ID);
    assert_eq!(
        node.parameters,
        HashMap::from([
            (
                SCATTER_VALUES_PARAM_ID,
                Expression::VariableId(names[".values"])
            ),
            (
                SCATTER_KEYS_PARAM_ID,
                Expression::Value(strings(&["anim/a"]))
            ),
            (
                SCATTER_WITHIN_PARAM_ID,
                Expression::Value(Value::String("anim".to_string()))
            ),
        ])
    );
    let mut by_id = names.iter().map(|(name, id)| (*id, name.clone())).collect();
    let exported = Node::try_from_tree_node(&node, &HashMap::new(), &mut by_id).unwrap();
    let written = String::from_utf8(BehaviorTree { root: exported }.to_groot_xml()).unwrap();
    for port in [r#"values="{.values}""#, r#"within="anim""#, "keys=\"json:"] {
        assert!(written.contains(port), "{written}");
    }
    let mut names = HashMap::new();
    let node = groot(r#"<Scatter values="{v}" keys="{k}"/>"#)
        .root
        .try_into_tree_node(&HashMap::new(), &mut names)
        .expect("`within` may be left out");
    assert!(!node.parameters.contains_key(&SCATTER_WITHIN_PARAM_ID));
    let mut by_id = names.iter().map(|(name, id)| (*id, name.clone())).collect();
    let exported = Node::try_from_tree_node(&node, &HashMap::new(), &mut by_id).unwrap();
    let written = String::from_utf8(BehaviorTree { root: exported }.to_groot_xml()).unwrap();
    assert!(!written.contains("within"), "{written}");

    let palette = include_str!("../../groot/groot_arora_palette.xml");
    let start = palette.find(r#"<Action ID="Scatter">"#).expect("Scatter");
    let end = start + palette[start..].find("</Action>").unwrap();
    let ports: Vec<&str> = palette[start..end]
        .lines()
        .skip(1)
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    assert_eq!(
        ports,
        [
            r#"<input_port name="values"/>"#,
            r#"<input_port name="keys"/>"#,
            r#"<input_port name="within"/>"#
        ]
    );
}
