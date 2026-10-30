//! Reading the main behavior back, and its revision: what `read` answers
//! after loads, edits and task runs, and when the revision key is written.
//!
//! The behavior is the trace tests' ([`trace_tests::behavior`]).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use arora_behavior::graph::{Binding, Graph, GraphDiff, Node, Port, Source};
use arora_behavior::{built_in, BehaviorContext, BehaviorInterpreter, LoadedBehavior, RunPolicy};
use arora_simple_data_store::SimpleDataStore;
use arora_types::call::Call;
use arora_types::data::{DataError, DataStore, Key, Slot, State, StateChange, Subscription};
use arora_types::value::{StructureField, Value};
use uuid::Uuid;

use crate::behavior::{graph_type, BehaviorTreeInterpreter};
use crate::call_expression_tests::{index, Engine, ADD, ADD_A, ADD_B, MATH, MUL, MUL_A, MUL_B};
use crate::nodes::{RUN_FUNCTION_ID, SEQ_FUNCTION_ID};
use crate::schema::_RET_PARAM_ID;
use crate::trace_tests::{behavior, key, value, Device, SUM};

/// The root of the graph a task run runs: a sequence adding 1 to `in`, then
/// running, so that the run stays alive.
const RUN_ROOT: Uuid = Uuid::from_u128(0x200);
/// The run's adding leaf.
const RUN_SUM: Uuid = Uuid::from_u128(0x201);

fn run_graph() -> Graph {
    Graph {
        root: Some(RUN_ROOT),
        nodes: vec![
            Node {
                id: RUN_ROOT,
                function: SEQ_FUNCTION_ID,
                children: Some(vec![RUN_SUM, Uuid::from_u128(0x202)]),
                ..Node::default()
            },
            Node {
                id: RUN_SUM,
                function: ADD,
                bind: [
                    (ADD_A, key("in")),
                    (ADD_B, value(1.0)),
                    (_RET_PARAM_ID, key("run/sum")),
                ]
                .into(),
                children: None,
            },
            Node {
                id: Uuid::from_u128(0x202),
                function: RUN_FUNCTION_ID,
                ..Node::default()
            },
        ],
    }
}

/// A call to add 1 and 2, spawned as a task run.
fn run_call() -> Call {
    let arg = |id, value| StructureField {
        id,
        value: Box::new(Value::F64(value)),
    };
    Call {
        module_id: Some(MATH),
        id: ADD,
        args: vec![arg(ADD_A, 1.0), arg(ADD_B, 2.0)],
    }
}

/// The edit that rebinds `sum`'s second addend: one of the main behavior's
/// nodes.
fn rebind_sum() -> GraphDiff {
    GraphDiff {
        bind: vec![Binding {
            node: SUM,
            port: ADD_B,
            source: value(2.0),
        }],
        ..GraphDiff::default()
    }
}

/// `graph` with `diff` applied.
fn applied(mut graph: Graph, diff: GraphDiff) -> Graph {
    graph.apply(diff).expect("the diff applies");
    graph
}

fn read(interpreter: &BehaviorTreeInterpreter) -> LoadedBehavior {
    interpreter.read().expect("the tree interpreter reads")
}

#[test]
fn nothing_loaded_reads_as_the_empty_graph_at_revision_zero() {
    let interpreter = BehaviorTreeInterpreter::new();
    assert_eq!(read(&interpreter), LoadedBehavior::default());
    assert_eq!(read(&interpreter).graph, Graph::empty());
}

#[test]
fn a_loaded_behavior_reads_as_loaded() {
    let mut interpreter = BehaviorTreeInterpreter::new();
    interpreter.load(behavior()).unwrap();
    assert_eq!(
        read(&interpreter),
        LoadedBehavior {
            revision: 1,
            graph: behavior(),
        }
    );
}

#[test]
fn an_edited_behavior_reads_as_edited() {
    let mut interpreter = BehaviorTreeInterpreter::new();
    interpreter.load(behavior()).unwrap();

    // A rebinding, and a node set beside the others.
    interpreter.apply(rebind_sum()).unwrap();
    let extra = GraphDiff {
        set: vec![Node {
            id: Uuid::from_u128(0x105),
            function: MUL,
            bind: [(MUL_A, key("in")), (MUL_B, value(3.0))].into(),
            children: None,
        }],
        ..GraphDiff::default()
    };
    interpreter.apply(extra.clone()).unwrap();
    let expected = applied(applied(behavior(), rebind_sum()), extra);
    assert_eq!(
        read(&interpreter),
        LoadedBehavior {
            revision: 3,
            graph: expected,
        }
    );

    // Removing the root unroots the behavior, as it unroots a graph.
    let unroot = GraphDiff {
        remove: vec![crate::trace_tests::ROOT],
        ..GraphDiff::default()
    };
    let expected = applied(read(&interpreter).graph, unroot.clone());
    interpreter.apply(unroot).unwrap();
    let unrooted = read(&interpreter);
    assert_eq!(unrooted.graph, expected);
    assert_eq!(unrooted.graph.root, None);
    assert_eq!(unrooted.revision, 4);
}

#[test]
fn task_runs_are_not_read_as_the_behavior() {
    let mut device = Device::new();
    let graph_run = device
        .interpreter
        .spawn_graph(&graph_type(), run_graph(), RunPolicy::Concurrent)
        .unwrap();
    let call_run = device
        .interpreter
        .spawn(run_call(), RunPolicy::Concurrent)
        .unwrap();
    device.tick();
    assert_eq!(
        device.read(&graph_run.status),
        Some(arora_behavior::Status::Running.into()),
        "the graph run is alive"
    );

    let loaded = read(&device.interpreter);
    assert_eq!(loaded.graph, behavior(), "neither the runner nor a run");
    assert_eq!(
        loaded.revision, 1,
        "spawning is not a change of the behavior"
    );

    // A run's nodes edited: the main behavior is as it was.
    device
        .interpreter
        .apply(GraphDiff {
            bind: vec![Binding {
                node: RUN_SUM,
                port: ADD_B,
                source: value(2.0),
            }],
            ..GraphDiff::default()
        })
        .unwrap();
    assert_eq!(read(&device.interpreter), loaded, "a run's edit");

    device.interpreter.halt(graph_run.id).unwrap();
    device.interpreter.halt(call_run.id).unwrap();
    device.tick();
    assert_eq!(read(&device.interpreter), loaded, "halting");
}

#[test]
fn a_read_behavior_loads_back_as_itself() {
    let mut device = Device::new();
    device.interpreter.apply(rebind_sum()).unwrap();
    device
        .interpreter
        .spawn_graph(&graph_type(), run_graph(), RunPolicy::Concurrent)
        .unwrap();
    device.tick();
    let loaded = read(&device.interpreter);

    // Loaded back beside the live run, it reads the same, one revision on.
    device.interpreter.load(loaded.graph.clone()).unwrap();
    device.tick();
    let again = read(&device.interpreter);
    assert_eq!(again.graph, loaded.graph);
    assert_eq!(again.revision, loaded.revision + 1);

    // And into another interpreter.
    let mut other = BehaviorTreeInterpreter::new();
    other.load(loaded.graph.clone()).unwrap();
    assert_eq!(read(&other).graph, loaded.graph);
}

#[test]
fn the_revision_counts_loads_and_accepted_edits_of_the_behavior() {
    let mut device = Device::new();
    let revision = |device: &Device| read(&device.interpreter).revision;
    assert_eq!(revision(&device), 1, "the load");

    device.interpreter.apply(rebind_sum()).unwrap();
    assert_eq!(revision(&device), 2, "an accepted edit");
    device.interpreter.apply(rebind_sum()).unwrap();
    assert_eq!(revision(&device), 2, "an edit that changes nothing");

    // A refused edit changes nothing.
    let refused = GraphDiff {
        bind: vec![Binding {
            node: Uuid::from_u128(0xdead),
            port: ADD_B,
            source: value(2.0),
        }],
        ..GraphDiff::default()
    };
    device.interpreter.apply(refused).unwrap_err();
    assert_eq!(revision(&device), 2, "a refused edit");

    // Nor do ticks, spawns and halts.
    device.tick();
    device.tick();
    let run = device
        .interpreter
        .spawn(run_call(), RunPolicy::Concurrent)
        .unwrap();
    device.tick();
    device.interpreter.halt(run.id).unwrap();
    device.tick();
    assert_eq!(revision(&device), 2, "ticks, a spawn and a halt");

    device.interpreter.load(behavior()).unwrap();
    assert_eq!(revision(&device), 3, "a load, even of the same behavior");
}

/// A store that counts the writes of the revision key.
#[derive(Clone, Default)]
struct Counting {
    store: SimpleDataStore,
    revisions: Arc<AtomicUsize>,
}

impl DataStore for Counting {
    fn read(&self, keys: &[Key]) -> Vec<Option<Value>> {
        self.store.read(keys)
    }

    fn write(&self, changes: StateChange) -> Result<(), DataError> {
        if changes
            .set
            .keys()
            .any(|key| key.path == built_in::BEHAVIOR_REVISION)
        {
            self.revisions.fetch_add(1, Ordering::Relaxed);
        }
        self.store.write(changes)
    }

    fn snapshot(&self) -> State {
        self.store.snapshot()
    }

    fn slot(&self, key: &Key) -> Box<dyn Slot> {
        self.store.slot(key)
    }

    fn subscribe(&self) -> Subscription {
        self.store.subscribe()
    }

    fn clone_box(&self) -> Box<dyn DataStore> {
        Box::new(self.clone())
    }
}

#[test]
fn the_revision_is_written_at_the_first_tick_and_once_per_change() {
    let store = Counting::default();
    let mut engine = Engine::default();
    let functions = index();
    for (name, input) in [("gate", 0.0), ("in", 10.0)] {
        store
            .write(StateChange::set(name, Value::F64(input)))
            .unwrap();
    }
    let mut interpreter = BehaviorTreeInterpreter::new();
    let mut tick = |interpreter: &mut BehaviorTreeInterpreter| {
        let mut ctx = BehaviorContext {
            store: &store,
            call_bridge: &mut engine,
            functions: &functions,
        };
        interpreter.tick(&mut ctx).expect("the tick succeeds");
    };
    let written = || {
        let value = store
            .read(&[Key::from(built_in::BEHAVIOR_REVISION)])
            .remove(0);
        (store.revisions.load(Ordering::Relaxed), value)
    };

    assert_eq!(written(), (0, None), "nothing before the first tick");
    tick(&mut interpreter);
    assert_eq!(written(), (1, Some(Value::U64(0))), "the first tick");
    tick(&mut interpreter);
    assert_eq!(written(), (1, Some(Value::U64(0))), "no change, no write");

    interpreter.load(behavior()).unwrap();
    assert_eq!(written(), (1, Some(Value::U64(0))), "written by the tick");
    tick(&mut interpreter);
    tick(&mut interpreter);
    assert_eq!(written(), (2, Some(Value::U64(1))), "a load, once");

    interpreter.apply(rebind_sum()).unwrap();
    interpreter
        .apply(GraphDiff {
            bind: vec![Binding {
                node: SUM,
                port: ADD_B,
                source: value(3.0),
            }],
            ..GraphDiff::default()
        })
        .unwrap();
    tick(&mut interpreter);
    tick(&mut interpreter);
    assert_eq!(
        written(),
        (3, Some(Value::U64(3))),
        "two edits between ticks, one write"
    );
}

/// The run graph's sequence, set with `children`.
fn run_root_with(children: Vec<Uuid>) -> Node {
    Node {
        id: RUN_ROOT,
        function: SEQ_FUNCTION_ID,
        children: Some(children),
        ..Node::default()
    }
}

/// A device running the behavior beside a live graph run.
fn device_with_a_run() -> Device {
    let mut device = Device::new();
    device
        .interpreter
        .spawn_graph(&graph_type(), run_graph(), RunPolicy::Concurrent)
        .unwrap();
    device.tick();
    device
}

/// An accepted edit that changes nothing a read answers leaves the
/// revision: an unbinding of a node the graph does not have, with or without
/// a behavior loaded.
#[test]
fn an_edit_that_changes_nothing_leaves_the_revision() {
    let unbind_nothing = GraphDiff {
        unbind: vec![Port::new(Uuid::from_u128(0xdead), ADD_B)],
        ..GraphDiff::default()
    };
    let mut empty = BehaviorTreeInterpreter::new();
    empty.apply(unbind_nothing.clone()).unwrap();
    assert_eq!(read(&empty), LoadedBehavior::default());

    let mut device = device_with_a_run();
    device.interpreter.apply(unbind_nothing).unwrap();
    assert_eq!(read(&device.interpreter).revision, 1);
}

/// An edit of a run that changes the main behavior advances the revision:
/// removing a run's node unbinds the main behavior's binding reading it, and
/// a node a run lets go of is the main behavior's.
#[test]
fn a_run_s_edit_that_changes_the_behavior_advances_the_revision() {
    let mut device = device_with_a_run();
    let reads_the_run = GraphDiff {
        bind: vec![Binding {
            node: SUM,
            port: ADD_B,
            source: Source::Port(Port::new(RUN_SUM, _RET_PARAM_ID)),
        }],
        ..GraphDiff::default()
    };
    device.interpreter.apply(reads_the_run).unwrap();
    let reading = read(&device.interpreter);
    assert_eq!(reading.revision, 2);

    // The binding is answered as it stands: the graph loads back beside the
    // run it reads.
    device.interpreter.load(reading.graph.clone()).unwrap();
    device.tick();
    assert_eq!(read(&device.interpreter).graph, reading.graph);
    assert_eq!(read(&device.interpreter).revision, 3);

    // The run's sum leaves: the binding reading it goes with it.
    device
        .interpreter
        .apply(GraphDiff {
            remove: vec![RUN_SUM],
            set: vec![run_root_with(vec![Uuid::from_u128(0x202)])],
            ..GraphDiff::default()
        })
        .unwrap();
    let unbound = read(&device.interpreter);
    assert_eq!(unbound.revision, 4);
    let expected = applied(
        behavior(),
        GraphDiff {
            unbind: vec![Port::new(SUM, ADD_B)],
            ..GraphDiff::default()
        },
    );
    assert_eq!(unbound.graph, expected, "the binding went with the node");

    // A node the run lets go of, and the main behavior holds no more than
    // before, is the main behavior's: a detached call.
    let mut device = device_with_a_run();
    device
        .interpreter
        .apply(GraphDiff {
            set: vec![run_root_with(vec![Uuid::from_u128(0x202)])],
            ..GraphDiff::default()
        })
        .unwrap();
    let released = read(&device.interpreter);
    assert_eq!(released.revision, 2);
    assert!(released.graph.node(&RUN_SUM).is_some(), "{released:?}");
}

/// A node the main behavior holds with a run is the main behavior's: a read
/// answers it, and the graph read loads back.
#[test]
fn a_node_held_with_a_run_reads_and_loads_back() {
    let mut device = device_with_a_run();
    device
        .interpreter
        .apply(GraphDiff {
            set: vec![run_root_with(vec![
                RUN_SUM,
                crate::trace_tests::ROOT,
                Uuid::from_u128(0x202),
            ])],
            ..GraphDiff::default()
        })
        .unwrap();
    let shared = read(&device.interpreter);
    assert_eq!(
        shared.graph,
        behavior(),
        "the shared root is the behavior's"
    );
    assert_eq!(shared.revision, 1, "the main behavior did not change");

    device.interpreter.load(shared.graph.clone()).unwrap();
    device.tick();
    let again = read(&device.interpreter);
    assert_eq!(again.graph, shared.graph);
    assert_eq!(again.revision, 2);

    // A node only the run holds is still refused.
    let mut taken = shared.graph;
    taken.nodes.push(run_graph().nodes[1].clone());
    let refused = device.interpreter.load(taken).unwrap_err();
    assert!(refused.message.contains("task run"), "{}", refused.message);
}
