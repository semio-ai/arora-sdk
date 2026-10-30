//! End to end: a remote spawns a behavior graph that steps a guest module every
//! tick and unpacks what it returns to store keys, edits the run, and halts it
//! — through the interpreter module's functions alone.

use std::time::Duration;

use arora_behavior::graph::{Binding, Graph, GraphDiff, Node, Source};
use arora_behavior::{interpreter_module, RunPolicy, Status};
use arora_behavior_tree::behavior::graph_type;
use arora_behavior_tree::nodes::{
    EQUAL_A_PARAM_ID, EQUAL_B_PARAM_ID, EQUAL_FUNCTION_ID, FALLBACK_FUNCTION_ID,
    PARALLEL_FUNCTION_ID, RUN_FUNCTION_ID, SEQ_FUNCTION_ID, SUCCEED_FUNCTION_ID,
    UNPACK_FUNCTION_ID, UNPACK_OUT_PARAM_IDS, UNPACK_VALUES_PARAM_ID,
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

// The guest's `frame(length, revision, time_ns, positioned_by: &mut Option<u64>)
// -> Vec<f64>`: it returns the values and writes the revision through
// `positioned_by`.
const FRAME: Uuid = uuid!("5b44c60c-0e52-432f-9a4c-9e14ff7edb12");
const FRAME_LENGTH: Uuid = uuid!("b1cce511-53dd-4754-9a15-71e0bafad3d5");
const FRAME_REVISION: Uuid = uuid!("a54ac423-d9f6-455c-9b3f-f8e802ef2316");
const FRAME_TIME_NS: Uuid = uuid!("e5a9f7c1-3b8d-4e26-9f0a-6c1d2b3e4f50");
const FRAME_POSITIONED_BY: Uuid = uuid!("8bc68040-2226-4e12-8f0c-0c947bcad035");

const ROOT: Uuid = Uuid::from_u128(0x1);
const STEP: Uuid = Uuid::from_u128(0x2);
const CALL: Uuid = Uuid::from_u128(0x3);
const GUARD: Uuid = Uuid::from_u128(0x4);
const OR_ELSE: Uuid = Uuid::from_u128(0x5);
const EQUAL: Uuid = Uuid::from_u128(0x6);
const UNPACK: Uuid = Uuid::from_u128(0x7);
const SKIP: Uuid = Uuid::from_u128(0x8);
const RUN: Uuid = Uuid::from_u128(0x9);
/// The frame's revision, the tree-local variable `.revision`.
const REVISION: &str = ".revision";
const VALUES: &str = ".values";

fn key(key: &str) -> Source {
    Source::Key(key.to_string())
}

/// The bindings of `Unpack`'s outputs to `keys`, an empty key leaving its
/// output unbound.
fn outputs(keys: &[&str]) -> Vec<Binding> {
    keys.iter()
        .enumerate()
        .filter(|(_, output)| !output.is_empty())
        .map(|(i, output)| Binding {
            node: UNPACK,
            port: UNPACK_OUT_PARAM_IDS[i],
            source: key(output),
        })
        .collect()
}

fn stepping_graph(keys: &[&str]) -> Graph {
    let node = |id, function, children: &[Uuid], bind: Vec<(Uuid, Source)>| Node {
        id,
        function,
        bind: bind.into_iter().collect(),
        children: (!children.is_empty()).then(|| children.to_vec()),
    };
    let mut graph = Graph {
        root: Some(ROOT),
        nodes: vec![
            node(ROOT, PARALLEL_FUNCTION_ID, &[STEP, RUN], vec![]),
            node(STEP, SEQ_FUNCTION_ID, &[CALL, OR_ELSE], vec![]),
            node(
                CALL,
                FRAME,
                &[],
                vec![
                    (FRAME_LENGTH, Source::Value(Value::U32(3))),
                    (FRAME_REVISION, Source::Value(Value::U64(1))),
                    (FRAME_TIME_NS, key("arora/time")),
                    (FRAME_POSITIONED_BY, key(REVISION)),
                    (_RET_PARAM_ID, key(VALUES)),
                ],
            ),
            node(OR_ELSE, FALLBACK_FUNCTION_ID, &[GUARD, SKIP], vec![]),
            node(GUARD, SEQ_FUNCTION_ID, &[EQUAL, UNPACK], vec![]),
            node(
                EQUAL,
                EQUAL_FUNCTION_ID,
                &[],
                vec![
                    (EQUAL_A_PARAM_ID, key(REVISION)),
                    (EQUAL_B_PARAM_ID, Source::Value(Value::U64(1))),
                ],
            ),
            node(
                UNPACK,
                UNPACK_FUNCTION_ID,
                &[],
                vec![(UNPACK_VALUES_PARAM_ID, key(VALUES))],
            ),
            node(SKIP, SUCCEED_FUNCTION_ID, &[], vec![]),
            node(RUN, RUN_FUNCTION_ID, &[], vec![]),
        ],
    };
    graph
        .apply(GraphDiff {
            bind: outputs(keys),
            ..GraphDiff::default()
        })
        .expect("Unpack is a node of the graph");
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
fn a_remote_spawns_a_stepping_graph_edits_its_outputs_and_halts_it() {
    let mut arora = Arora::builder()
        .with_declared_module::<Module>(
            Executor {
                name: "wasm-component".to_string(),
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
    assert_eq!(
        read(&arora, &[".values", ".revision"]),
        vec![None, None],
        "the intermediates are tree-local"
    );
    step(&mut arora);
    assert_eq!(
        read(&arora, &["a/0"]),
        vec![Some(Value::F64(seconds(&arora)))],
        "written every step"
    );

    // The module's revision moves on: the guard holds every write.
    let edit = |diff: GraphDiff| interpreter_module::encode_edit(&diff);
    let rebind = |node, port, value| GraphDiff {
        bind: vec![Binding {
            node,
            port,
            source: Source::Value(value),
        }],
        ..GraphDiff::default()
    };
    settle(
        &mut arora,
        remote.call(edit(rebind(CALL, FRAME_REVISION, Value::U64(2)))),
    )
    .expect("the edit applies");
    let held = read(&arora, &["a/0", "a/2"]);
    step(&mut arora);
    assert_eq!(read(&arora, &["a/0", "a/2"]), held, "nothing written");

    // One edit rebinds the outputs and the revision they are positioned by.
    let mut outputs_moved = rebind(EQUAL, EQUAL_B_PARAM_ID, Value::U64(2));
    outputs_moved.bind.extend(outputs(&["b/0", "b/1", "b/2"]));
    settle(&mut arora, remote.call(edit(outputs_moved))).expect("the edit applies");
    step(&mut arora);
    let now = seconds(&arora);
    assert_eq!(
        read(&arora, &["b/0", "b/1", "b/2"]),
        vec![
            Some(Value::F64(now)),
            Some(Value::F64(now + 1.0)),
            Some(Value::F64(now + 2.0)),
        ],
        "the new keys are written"
    );
    assert_eq!(read(&arora, &["a/0", "a/2"]), held, "the old keys are not");

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
