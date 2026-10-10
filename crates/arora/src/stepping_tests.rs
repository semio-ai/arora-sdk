//! End to end: a remote spawns a behavior graph that steps a guest module every
//! tick and writes what it returns under a key table, edits the run, and halts
//! it — through the interpreter module's functions alone.

use std::time::Duration;

use arora_behavior::graph::{Graph, GraphDiff, Link, LinkSource, Node, Port};
use arora_behavior::{interpreter_module, RunPolicy, Status};
use arora_behavior_tree::behavior::graph_type;
use arora_behavior_tree::nodes::{
    EQUAL_A_PARAM_ID, EQUAL_B_PARAM_ID, EQUAL_FUNCTION_ID, FALLBACK_FUNCTION_ID,
    PARALLEL_FUNCTION_ID, RUN_FUNCTION_ID, SEQ_FUNCTION_ID, SUCCEED_FUNCTION_ID,
    WRITE_KEYS_FUNCTION_ID, WRITE_KEYS_KEYS_PARAM_ID, WRITE_KEYS_VALUES_PARAM_ID,
};
use arora_behavior_tree::schema::_RET_PARAM_ID;
use arora_bridge::Caller;
use arora_types::call::Call;
use arora_types::data::Key;
use arora_types::module::low::Executor;
use arora_types::value::Value;
use test_rust_wasm::test_rust_wasm::Module;
use uuid::{uuid, Uuid};

use crate::caller_tests::settle;
use crate::Arora;

const WASM: &[u8] = include_bytes!(env!("CARGO_CDYLIB_FILE_TEST_RUST_WASM_test_rust_wasm"));

// The guest's `frame(length, revision, time_ns) -> Frame { revision, values }`.
const FRAME: Uuid = uuid!("5b44c60c-0e52-432f-9a4c-9e14ff7edb12");
const FRAME_LENGTH: Uuid = uuid!("b1cce511-53dd-4754-9a15-71e0bafad3d5");
const FRAME_REVISION: Uuid = uuid!("a54ac423-d9f6-455c-9b3f-f8e802ef2316");
const FRAME_TIME_NS: Uuid = uuid!("e5a9f7c1-3b8d-4e26-9f0a-6c1d2b3e4f50");
const FRAME_REVISION_FIELD: Uuid = uuid!("9340674c-b0b0-4ffc-96ae-f207b39dc09c");
const FRAME_VALUES_FIELD: Uuid = uuid!("b629962e-57c6-4fdf-bf8c-a5b0065bc1ca");

const ROOT: Uuid = Uuid::from_u128(0x1);
const STEP: Uuid = Uuid::from_u128(0x2);
const CALL: Uuid = Uuid::from_u128(0x3);
const GUARD: Uuid = Uuid::from_u128(0x4);
const OR_ELSE: Uuid = Uuid::from_u128(0x5);
const EQUAL: Uuid = Uuid::from_u128(0x6);
const WRITE: Uuid = Uuid::from_u128(0x7);
const SKIP: Uuid = Uuid::from_u128(0x8);
const RUN: Uuid = Uuid::from_u128(0x9);
/// The frame, a tree-local variable: `graph.variables` does not name it.
const FRAME_VAR: Uuid = Uuid::from_u128(0xF0);
/// The clock, the store's `arora/time`.
const TIME_VAR: Uuid = Uuid::from_u128(0xF1);

fn keys(keys: &[&str]) -> Value {
    Value::ArrayString(keys.iter().map(|key| key.to_string()).collect())
}

fn select(field: Uuid) -> LinkSource {
    LinkSource::Select {
        source: Box::new(LinkSource::Variable(FRAME_VAR)),
        path: Key::new(format!(".{field}")),
    }
}

/// The stepping graph:
///
/// ```text
/// Parallel
/// ├─ Sequence
/// │  ├─ frame(length 3, revision 1, time_ns ← arora/time)   → frame
/// │  └─ Fallback
/// │     ├─ Sequence
/// │     │  ├─ Equal(frame.revision, 1)
/// │     │  └─ WriteKeys(table, frame.values)
/// │     └─ Succeed
/// └─ Run
/// ```
fn stepping_graph(table: &[&str]) -> Graph {
    let node = |id, function, children: &[Uuid]| Node {
        id,
        function,
        children: (!children.is_empty()).then(|| children.to_vec()),
        ..Node::default()
    };
    let mut graph = Graph::empty();
    graph.root = Some(ROOT);
    for node in [
        node(ROOT, PARALLEL_FUNCTION_ID, &[STEP, RUN]),
        node(STEP, SEQ_FUNCTION_ID, &[CALL, OR_ELSE]),
        node(CALL, FRAME, &[]),
        node(OR_ELSE, FALLBACK_FUNCTION_ID, &[GUARD, SKIP]),
        node(GUARD, SEQ_FUNCTION_ID, &[EQUAL, WRITE]),
        node(EQUAL, EQUAL_FUNCTION_ID, &[]),
        node(WRITE, WRITE_KEYS_FUNCTION_ID, &[]),
        node(SKIP, SUCCEED_FUNCTION_ID, &[]),
        node(RUN, RUN_FUNCTION_ID, &[]),
    ] {
        graph.nodes.insert(node.id, node);
    }
    let link = |node, port, source| Link::new(Port::new(node, port), source);
    graph.links = vec![
        link(CALL, FRAME_LENGTH, LinkSource::Literal(Value::U32(3))),
        link(CALL, FRAME_REVISION, LinkSource::Literal(Value::U64(1))),
        link(CALL, FRAME_TIME_NS, LinkSource::Variable(TIME_VAR)),
        link(CALL, _RET_PARAM_ID, LinkSource::Variable(FRAME_VAR)),
        link(EQUAL, EQUAL_A_PARAM_ID, select(FRAME_REVISION_FIELD)),
        link(EQUAL, EQUAL_B_PARAM_ID, LinkSource::Literal(Value::U64(1))),
        link(
            WRITE,
            WRITE_KEYS_KEYS_PARAM_ID,
            LinkSource::Literal(keys(table)),
        ),
        link(
            WRITE,
            WRITE_KEYS_VALUES_PARAM_ID,
            select(FRAME_VALUES_FIELD),
        ),
    ];
    graph.variables.insert(TIME_VAR, "arora/time".to_string());
    graph
}

fn read(arora: &Arora, keys: &[&str]) -> Vec<Option<Value>> {
    let keys: Vec<Key> = keys.iter().map(|key| Key::new(*key)).collect();
    arora.store().read(&keys)
}

fn seconds(arora: &Arora) -> f64 {
    match arora.store().read(&[Key::new("arora/time")]).remove(0) {
        Some(Value::U64(ns)) => ns as f64 / 1e9,
        other => panic!("arora/time is a u64, not {other:?}"),
    }
}

#[test]
fn a_remote_spawns_a_stepping_graph_edits_its_table_and_halts_it() {
    let mut arora = Arora::builder()
        .with_declared_module::<Module>(
            Executor {
                name: "wasm".to_string(),
                min_version: None,
                max_version: None,
            },
            WASM.to_vec(),
        )
        .build()
        .expect("a device carrying the guest");
    let remote = arora.caller();
    let step = |arora: &mut Arora| arora.step(Duration::from_millis(10)).expect("step");

    // Spawn: the Call a remote sends over a bridge.
    let spawned = settle(
        &mut arora,
        remote.call(interpreter_module::encode_spawn_graph(
            &graph_type(),
            &stepping_graph(&["a/0", "", "a/2"]),
            RunPolicy::Concurrent,
        )),
    )
    .expect("the graph spawns");
    let handle = interpreter_module::decode_spawn_result(&spawned.ret).expect("a handle");
    step(&mut arora);
    let now = seconds(&arora);
    assert_eq!(
        read(&arora, &["a/0", "a/2", handle.status.path.as_str()]),
        vec![
            Some(Value::F64(now)),
            Some(Value::F64(now + 2.0)),
            Some(Status::Running.into()),
        ],
        "each value under its key, the run running"
    );
    assert_eq!(arora.store().read(&[Key::new("")]), vec![None]);
    step(&mut arora);
    assert_eq!(
        read(&arora, &["a/0"]),
        vec![Some(Value::F64(seconds(&arora)))],
        "written every step"
    );

    // The module's revision moves on: the guard holds every write.
    let edit = |diff: GraphDiff| interpreter_module::encode_edit(&diff);
    let relink = |node, port, value| GraphDiff {
        add_links: vec![Link::new(Port::new(node, port), LinkSource::Literal(value))],
        ..GraphDiff::default()
    };
    settle(
        &mut arora,
        remote.call(edit(relink(CALL, FRAME_REVISION, Value::U64(2)))),
    )
    .expect("the edit applies");
    let held = read(&arora, &["a/0", "a/2"]);
    step(&mut arora);
    assert_eq!(read(&arora, &["a/0", "a/2"]), held, "nothing written");

    // One edit swaps the table and the revision it is positioned by.
    let mut retable = relink(EQUAL, EQUAL_B_PARAM_ID, Value::U64(2));
    retable.add_links.push(Link::new(
        Port::new(WRITE, WRITE_KEYS_KEYS_PARAM_ID),
        LinkSource::Literal(keys(&["b/0", "b/1", "b/2"])),
    ));
    settle(&mut arora, remote.call(edit(retable))).expect("the edit applies");
    step(&mut arora);
    let now = seconds(&arora);
    assert_eq!(
        read(&arora, &["b/0", "b/1", "b/2"]),
        vec![
            Some(Value::F64(now)),
            Some(Value::F64(now + 1.0)),
            Some(Value::F64(now + 2.0)),
        ],
        "the new table is written"
    );
    assert_eq!(read(&arora, &["a/0", "a/2"]), held, "the old table is not");

    // Halt with the handle's own stop call.
    let stop: Call = handle.stop.clone();
    settle(&mut arora, remote.call(stop)).expect("the halt applies");
    step(&mut arora);
    let halted = read(&arora, &["b/0"]);
    step(&mut arora);
    assert_eq!(
        read(&arora, &[handle.status.path.as_str()]),
        vec![Some(Status::Failure.into())],
        "a halted run ends Failure"
    );
    assert_eq!(
        read(&arora, &["b/0"]),
        halted,
        "a halted run writes nothing"
    );
}
