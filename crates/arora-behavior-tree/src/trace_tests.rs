//! Traces of the interpreter: what each report states, tick by tick.
//!
//! The behavior is `sequence(gate, sum, near)`: `gate` succeeds while the key
//! `gate` is below 0.5, `sum` writes `in + 1` to the key `sum`, and `near`
//! succeeds while `double`, a detached node computing `in * 2`, is below 50.
//! The functions are the fake engine's ([`call_expression_tests`](crate::call_expression_tests)).

use std::collections::BTreeMap;

use arora_behavior::graph::{Binding, Graph, GraphDiff, Node, Port, Source};
use arora_behavior::trace::decode_report;
use arora_behavior::{
    built_in, Activity, BehaviorContext, BehaviorInterpreter, NodeStatus, Status, TaskHandle,
    TraceOptions, TraceReport,
};
use arora_simple_data_store::SimpleDataStore;
use arora_types::data::{DataStore, Key, StateChange};
use arora_types::value::Value;
use uuid::Uuid;

use crate::behavior::BehaviorTreeInterpreter;
use crate::call_expression_tests::{
    index, Engine, ADD, ADD_A, ADD_B, BELOW, BELOW_LIMIT, BELOW_X, MUL, MUL_A, MUL_B, SMOOTH,
    SMOOTH_ALPHA, SMOOTH_STATE, SMOOTH_X,
};
use crate::nodes::SEQ_FUNCTION_ID;
use crate::schema::_RET_PARAM_ID;

const ROOT: Uuid = Uuid::from_u128(0x100);
const GATE: Uuid = Uuid::from_u128(0x101);
const SUM: Uuid = Uuid::from_u128(0x102);
const NEAR: Uuid = Uuid::from_u128(0x103);
const DOUBLE: Uuid = Uuid::from_u128(0x104);

fn value(value: f64) -> Source {
    Source::Value(Value::F64(value))
}

fn key(key: &str) -> Source {
    Source::Key(key.to_string())
}

fn behavior() -> Graph {
    let node = |id, function, bind: Vec<(Uuid, Source)>| Node {
        id,
        function,
        bind: bind.into_iter().collect(),
        children: None,
    };
    Graph {
        root: Some(ROOT),
        nodes: vec![
            Node {
                id: ROOT,
                function: SEQ_FUNCTION_ID,
                children: Some(vec![GATE, SUM, NEAR]),
                ..Node::default()
            },
            node(
                GATE,
                BELOW,
                vec![(BELOW_X, key("gate")), (BELOW_LIMIT, value(0.5))],
            ),
            node(
                SUM,
                ADD,
                vec![
                    (ADD_A, key("in")),
                    (ADD_B, value(1.0)),
                    (_RET_PARAM_ID, key("sum")),
                ],
            ),
            node(
                NEAR,
                BELOW,
                vec![
                    (BELOW_X, Source::Port(Port::new(DOUBLE, _RET_PARAM_ID))),
                    (BELOW_LIMIT, value(50.0)),
                ],
            ),
            node(DOUBLE, MUL, vec![(MUL_A, key("in")), (MUL_B, value(2.0))]),
        ],
    }
}

/// A device in miniature: the interpreter, its store and its engine.
struct Device {
    interpreter: BehaviorTreeInterpreter,
    store: SimpleDataStore,
    engine: Engine,
    functions: arora_behavior::FunctionIndex,
}

impl Device {
    fn new() -> Self {
        Self::running(behavior())
    }

    fn running(graph: Graph) -> Self {
        let mut interpreter = BehaviorTreeInterpreter::new();
        interpreter.load(graph).expect("the behavior loads");
        let device = Self {
            interpreter,
            store: SimpleDataStore::new(),
            engine: Engine::default(),
            functions: index(),
        };
        device.set("gate", 0.0);
        device.set("in", 10.0);
        device
    }

    fn set(&self, name: &str, value: f64) {
        self.store
            .write(StateChange::set(name, Value::F64(value)))
            .expect("the store writes");
    }

    fn tick(&mut self) {
        let mut ctx = BehaviorContext {
            store: &self.store,
            call_bridge: &mut self.engine,
            functions: &self.functions,
        };
        self.interpreter.tick(&mut ctx).expect("the tick succeeds");
    }

    fn read(&self, key: &Key) -> Option<Value> {
        self.store.read(std::slice::from_ref(key)).remove(0)
    }

    /// The report on `trace`'s feedback key.
    fn report(&self, trace: &TaskHandle) -> Option<TraceReport> {
        self.read(&trace.feedback[0])
            .map(|value| decode_report(&value).expect("a report"))
    }

    /// The report `trace` wrote at tick `tick`; `None` when it wrote none
    /// then.
    fn report_at(&self, trace: &TaskHandle, tick: u64) -> Option<TraceReport> {
        self.report(trace).filter(|report| report.tick == tick)
    }

    fn status(&self, trace: &TaskHandle) -> Option<Status> {
        self.read(&trace.status)
            .map(|value| Status::try_from(value).expect("a status"))
    }
}

/// The statuses `report` states of the behavior's nodes, the scaffold's
/// left out.
fn statuses(report: &TraceReport) -> BTreeMap<Uuid, NodeStatus> {
    report
        .status
        .iter()
        .filter(|(node, _)| [ROOT, GATE, SUM, NEAR].contains(node))
        .map(|(node, status)| (*node, *status))
        .collect()
}

/// The activities `report` states, by node.
fn activity(report: &TraceReport) -> BTreeMap<Uuid, Activity> {
    report.activity.iter().copied().collect()
}

/// The values `report` states, by node then port.
fn values(report: &TraceReport) -> BTreeMap<Uuid, BTreeMap<Uuid, Value>> {
    let mut by_node: BTreeMap<Uuid, BTreeMap<Uuid, Value>> = BTreeMap::new();
    for value in &report.values {
        by_node
            .entry(value.node)
            .or_default()
            .insert(value.port, value.value.clone());
    }
    by_node
}

/// The status `report` states of `node`.
fn status_of(report: &TraceReport, node: Uuid) -> Option<NodeStatus> {
    report
        .status
        .iter()
        .find(|(id, _)| *id == node)
        .map(|(_, status)| *status)
}

#[test]
fn a_trace_reports_the_first_tick_then_transitions() {
    let mut device = Device::new();
    let trace = device
        .interpreter
        .trace(TraceOptions::default())
        .expect("the tree interpreter traces");
    assert!(
        trace.status.path.starts_with(&format!(
            "arora/tasks/{}/{}/{}/",
            arora_behavior::interpreter_module::ID,
            arora_behavior::interpreter_module::TRACE,
            trace.id.0
        )),
        "{}",
        trace.status.path
    );
    assert_eq!(device.report(&trace), None, "nothing before a tick");

    // The first tick: every node it reached.
    device.tick();
    assert_eq!(device.status(&trace), Some(Status::Running));
    let first = device.report_at(&trace, 1).expect("a first report");
    assert_eq!(first.since, None, "a first report follows none");
    assert_eq!(
        statuses(&first),
        BTreeMap::from([
            (ROOT, NodeStatus::Success),
            (GATE, NodeStatus::Success),
            (SUM, NodeStatus::Success),
            (NEAR, NodeStatus::Success),
        ])
    );
    assert_eq!(
        first.status.len(),
        5,
        "the behavior's four nodes and the runner: {first:?}"
    );
    assert!(first.values.is_empty(), "no value is watched");

    // A tick that changes no status reports none: here only the detached
    // node's activity changed, from called to cached.
    device.tick();
    let second = device.report_at(&trace, 2).expect("an activity change");
    assert!(second.status.is_empty(), "{second:?}");
    assert_eq!(second.since, Some(1));

    // A tick that changes nothing writes nothing.
    device.tick();
    assert_eq!(device.report_at(&trace, 3), None);

    // The gate closes: the nodes after it are not reached.
    device.set("gate", 1.0);
    device.tick();
    let closed = device.report_at(&trace, 4).expect("a report of the change");
    assert_eq!(closed.since, Some(2), "tick 3 changed nothing");
    assert_eq!(
        statuses(&closed),
        BTreeMap::from([
            (ROOT, NodeStatus::Failure),
            (GATE, NodeStatus::Failure),
            (SUM, NodeStatus::NotReached),
            (NEAR, NodeStatus::NotReached),
        ])
    );

    // It opens on a far value: only what differs from the last report.
    device.set("gate", 0.0);
    device.set("in", 30.0);
    device.tick();
    let far = device.report_at(&trace, 5).expect("a report of the change");
    assert_eq!(
        statuses(&far),
        BTreeMap::from([
            (GATE, NodeStatus::Success),
            (SUM, NodeStatus::Success),
            (NEAR, NodeStatus::Failure),
        ]),
        "the root failed before and fails again"
    );
}

#[test]
fn a_trace_reports_detached_activity() {
    let mut device = Device::new();
    let trace = device.interpreter.trace(TraceOptions::default()).unwrap();

    // Read for the first time: called.
    device.tick();
    let first = device.report_at(&trace, 1).unwrap();
    assert_eq!(
        activity(&first),
        BTreeMap::from([(DOUBLE, Activity::Called)])
    );

    // Read again, its input unchanged: cached.
    device.tick();
    let cached = device.report_at(&trace, 2).unwrap();
    assert_eq!(
        activity(&cached),
        BTreeMap::from([(DOUBLE, Activity::Cached)])
    );
    assert!(cached.status.is_empty(), "no status changed: {cached:?}");

    // Still cached: no change, no report.
    device.tick();
    assert_eq!(device.report_at(&trace, 3), None);

    // Its reader is not reached: idle.
    device.set("gate", 1.0);
    device.tick();
    let idle = device.report_at(&trace, 4).unwrap();
    assert_eq!(activity(&idle), BTreeMap::from([(DOUBLE, Activity::Idle)]));

    // Read again with a new input: called.
    device.set("gate", 0.0);
    device.set("in", 11.0);
    device.tick();
    let called = device.report_at(&trace, 5).unwrap();
    assert_eq!(
        activity(&called),
        BTreeMap::from([(DOUBLE, Activity::Called)])
    );
}

#[test]
fn a_trace_reports_watched_values_when_they_change() {
    let mut device = Device::new();
    let trace = device
        .interpreter
        .trace(TraceOptions {
            values: vec![SUM, DOUBLE],
            min_interval_ns: None,
        })
        .unwrap();

    device.tick();
    let first = device.report_at(&trace, 1).unwrap();
    assert_eq!(
        values(&first),
        BTreeMap::from([
            (SUM, BTreeMap::from([(_RET_PARAM_ID, Value::F64(11.0))])),
            (DOUBLE, BTreeMap::from([(_RET_PARAM_ID, Value::F64(20.0))])),
        ])
    );

    // Unchanged values are not reported again.
    device.tick();
    assert!(device.report_at(&trace, 2).unwrap().values.is_empty());

    // A new input: both change.
    device.set("in", 12.0);
    device.tick();
    assert_eq!(
        values(&device.report_at(&trace, 3).unwrap()),
        BTreeMap::from([
            (SUM, BTreeMap::from([(_RET_PARAM_ID, Value::F64(13.0))])),
            (DOUBLE, BTreeMap::from([(_RET_PARAM_ID, Value::F64(24.0))])),
        ])
    );

    // A trace watching nothing never reports a value.
    let mut unwatched = Device::new();
    let statuses_only = unwatched
        .interpreter
        .trace(TraceOptions::default())
        .unwrap();
    for input in [10.0, 12.0, 14.0] {
        unwatched.set("in", input);
        unwatched.tick();
        let report = unwatched.report(&statuses_only).unwrap();
        assert!(report.values.is_empty(), "{report:?}");
    }
}

#[test]
fn concurrent_traces_report_under_their_own_options() {
    let mut device = Device::new();
    let every_tick = device.interpreter.trace(TraceOptions::default()).unwrap();
    let paced = device
        .interpreter
        .trace(TraceOptions {
            values: vec![SUM],
            min_interval_ns: Some(2_000),
        })
        .unwrap();
    assert_ne!(every_tick.id, paced.id);
    assert_ne!(every_tick.feedback, paced.feedback);
    let tick_at = |device: &mut Device, time: u64| {
        device
            .store
            .write(StateChange::set(built_in::TIME, Value::U64(time)))
            .unwrap();
        device.tick();
    };

    tick_at(&mut device, 1_000);
    assert!(device.report_at(&every_tick, 1).unwrap().values.is_empty());
    assert_eq!(
        values(&device.report_at(&paced, 1).unwrap())[&SUM][&_RET_PARAM_ID],
        Value::F64(11.0)
    );

    // Within the interval: the paced trace holds its changes.
    device.set("gate", 1.0);
    tick_at(&mut device, 2_000);
    assert_eq!(
        statuses(&device.report_at(&every_tick, 2).unwrap())[&SUM],
        NodeStatus::NotReached
    );
    assert_eq!(device.report_at(&paced, 2), None, "not due");

    // Due: what changed since its last report, as it stands now. The gate
    // closed and opened again, and the input changed meanwhile.
    device.set("gate", 0.0);
    device.set("in", 30.0);
    tick_at(&mut device, 3_000);
    let report = device.report_at(&paced, 3).expect("due");
    assert_eq!(
        statuses(&report),
        BTreeMap::from([(NEAR, NodeStatus::Failure), (ROOT, NodeStatus::Failure)]),
        "the gate and sum are as last reported"
    );
    assert_eq!(values(&report)[&SUM][&_RET_PARAM_ID], Value::F64(31.0));
    assert_eq!(
        statuses(&device.report_at(&every_tick, 3).unwrap()),
        BTreeMap::from([
            (GATE, NodeStatus::Success),
            (SUM, NodeStatus::Success),
            (NEAR, NodeStatus::Failure),
        ])
    );
}

#[test]
fn halting_a_trace_ends_it() {
    let mut device = Device::new();
    let halted = device.interpreter.trace(TraceOptions::default()).unwrap();
    let kept = device.interpreter.trace(TraceOptions::default()).unwrap();
    device.tick();
    assert_eq!(device.status(&halted), Some(Status::Running));

    device.interpreter.halt(halted.id).expect("halt");
    device.set("gate", 1.0);
    device.tick();
    assert_eq!(device.status(&halted), Some(Status::Failure), "terminal");
    let last = device.report(&halted);
    assert_eq!(last.as_ref().map(|report| report.tick), Some(1));
    assert!(
        device.report_at(&kept, 2).is_some(),
        "the other trace goes on"
    );

    // Its keys change no more.
    device.set("gate", 0.0);
    device.tick();
    assert_eq!(device.status(&halted), Some(Status::Failure));
    assert_eq!(device.report(&halted), last);

    // Idempotent, as for a run.
    device
        .interpreter
        .halt(halted.id)
        .expect("halting a halted trace is a no-op");

    // The last trace halted, the interpreter records nothing.
    device.interpreter.halt(kept.id).unwrap();
    device.tick();
    assert_eq!(device.status(&kept), Some(Status::Failure));
    assert!(!device.interpreter.records());
}

#[test]
fn a_trace_survives_an_edit_and_reports_the_restarted_nodes() {
    let mut device = Device::new();
    let trace = device
        .interpreter
        .trace(TraceOptions {
            values: vec![SUM],
            min_interval_ns: None,
        })
        .unwrap();
    device.tick();
    device.tick();

    // Rebinding the gate's limit restarts the gate, whose status is the same.
    device
        .interpreter
        .apply(GraphDiff {
            bind: vec![Binding {
                node: GATE,
                port: BELOW_LIMIT,
                source: value(0.7),
            }],
            ..GraphDiff::default()
        })
        .expect("the edit applies");
    device.tick();
    let report = device
        .report_at(&trace, 3)
        .expect("a report after the edit");
    assert_eq!(report.restarted, vec![GATE]);
    assert_eq!(
        statuses(&report),
        BTreeMap::from([(GATE, NodeStatus::Success)])
    );

    // Setting the sum node whole restarts it: its value is stated anew.
    let mut sum = behavior()
        .nodes
        .into_iter()
        .find(|node| node.id == SUM)
        .unwrap();
    sum.bind.insert(ADD_B, value(1.0));
    device
        .interpreter
        .apply(GraphDiff {
            set: vec![sum],
            ..GraphDiff::default()
        })
        .unwrap();
    device.tick();
    let report = device.report_at(&trace, 4).unwrap();
    assert_eq!(report.restarted, vec![SUM]);
    assert_eq!(values(&report)[&SUM][&_RET_PARAM_ID], Value::F64(11.0));

    // Then on as before: the detached node, called again by the new
    // lowering, answers from its cache, and then nothing changes.
    device.tick();
    let after = device.report_at(&trace, 5).unwrap();
    assert!(
        after.restarted.is_empty() && after.status.is_empty(),
        "{after:?}"
    );
    device.tick();
    assert_eq!(device.report_at(&trace, 6), None);
    assert_eq!(device.status(&trace), Some(Status::Running));
}

/// A watched node's outputs are what its cells hold, and its return value
/// when no binding keeps it: here an attached `add` whose return is bound to
/// nothing, and a detached `smooth` whose state is a parameter it writes.
#[test]
fn a_trace_reports_unbound_returns_and_written_parameters() {
    const SMOOTHED: Uuid = Uuid::from_u128(0x105);
    let graph = Graph {
        root: Some(ROOT),
        nodes: vec![
            Node {
                id: ROOT,
                function: SEQ_FUNCTION_ID,
                children: Some(vec![SUM]),
                ..Node::default()
            },
            Node {
                id: SUM,
                function: ADD,
                bind: [(ADD_A, key("in")), (ADD_B, value(1.0))].into(),
                children: None,
            },
            Node {
                id: SMOOTHED,
                function: SMOOTH,
                bind: [
                    (SMOOTH_X, key("in")),
                    (SMOOTH_ALPHA, value(0.5)),
                    (SMOOTH_STATE, value(0.0)),
                    (_RET_PARAM_ID, key("smoothed")),
                ]
                .into(),
                children: None,
            },
        ],
    };
    let mut device = Device::running(graph);
    let trace = device
        .interpreter
        .trace(TraceOptions {
            values: vec![SUM, SMOOTHED],
            min_interval_ns: None,
        })
        .unwrap();
    device.tick();
    let first = device.report_at(&trace, 1).unwrap();
    assert_eq!(
        values(&first),
        BTreeMap::from([
            (SUM, BTreeMap::from([(_RET_PARAM_ID, Value::F64(11.0))])),
            (
                SMOOTHED,
                BTreeMap::from([
                    (_RET_PARAM_ID, Value::F64(5.0)),
                    (SMOOTH_STATE, Value::F64(5.0)),
                ])
            ),
        ])
    );
    assert_eq!(status_of(&first, SUM), Some(NodeStatus::Success));

    // The state settles toward the input; the sum holds.
    device.tick();
    assert_eq!(
        values(&device.report_at(&trace, 2).unwrap()),
        BTreeMap::from([(
            SMOOTHED,
            BTreeMap::from([
                (_RET_PARAM_ID, Value::F64(7.5)),
                (SMOOTH_STATE, Value::F64(7.5)),
            ])
        )])
    );
}

/// A node removed and set again under its id starts with no return: the
/// value its former self returned is not reported for it.
#[test]
fn a_node_set_again_reports_no_former_return() {
    let sum = || Node {
        id: SUM,
        function: ADD,
        bind: [(ADD_A, key("in")), (ADD_B, value(1.0))].into(),
        children: None,
    };
    let root = |children: Vec<Uuid>| Node {
        id: ROOT,
        function: SEQ_FUNCTION_ID,
        children: Some(children),
        ..Node::default()
    };
    let gate = behavior()
        .nodes
        .into_iter()
        .find(|node| node.id == GATE)
        .unwrap();
    let mut device = Device::running(Graph {
        root: Some(ROOT),
        nodes: vec![root(vec![GATE, SUM]), gate, sum()],
    });
    let trace = device
        .interpreter
        .trace(TraceOptions {
            values: vec![SUM],
            min_interval_ns: None,
        })
        .unwrap();
    device.tick();
    assert_eq!(
        values(&device.report_at(&trace, 1).unwrap())[&SUM][&_RET_PARAM_ID],
        Value::F64(11.0)
    );

    // Removed: its return is unset.
    device
        .interpreter
        .apply(GraphDiff {
            remove: vec![SUM],
            set: vec![root(vec![GATE])],
            ..GraphDiff::default()
        })
        .unwrap();
    device.tick();
    assert_eq!(
        values(&device.report_at(&trace, 2).unwrap())[&SUM][&_RET_PARAM_ID],
        Value::Unit
    );

    // Set again behind a closed gate: never ticked, it returned nothing.
    device.set("gate", 1.0);
    device
        .interpreter
        .apply(GraphDiff {
            set: vec![sum(), root(vec![GATE, SUM])],
            ..GraphDiff::default()
        })
        .unwrap();
    device.tick();
    let report = device.report_at(&trace, 3).unwrap();
    assert!(report.values.is_empty(), "{report:?}");
}

/// A node no trace watches forgets what it returned: a later trace watching
/// it does not report a value it returned while unwatched.
#[test]
fn an_unwatched_node_forgets_its_return() {
    let gate = behavior()
        .nodes
        .into_iter()
        .find(|node| node.id == GATE)
        .unwrap();
    let mut device = Device::running(Graph {
        root: Some(ROOT),
        nodes: vec![
            Node {
                id: ROOT,
                function: SEQ_FUNCTION_ID,
                children: Some(vec![GATE, SUM]),
                ..Node::default()
            },
            gate,
            Node {
                id: SUM,
                function: ADD,
                bind: [(ADD_A, key("in")), (ADD_B, value(1.0))].into(),
                children: None,
            },
        ],
    });
    let watch = TraceOptions {
        values: vec![SUM],
        min_interval_ns: None,
    };
    let watching = device.interpreter.trace(watch.clone()).unwrap();
    device.interpreter.trace(TraceOptions::default()).unwrap();
    device.tick();
    assert_eq!(
        values(&device.report_at(&watching, 1).unwrap())[&SUM][&_RET_PARAM_ID],
        Value::F64(11.0)
    );

    // Unwatched, it returns another value, then stops being reached.
    device.interpreter.halt(watching.id).unwrap();
    device.set("in", 20.0);
    device.tick();
    device.set("gate", 1.0);
    let later = device.interpreter.trace(watch).unwrap();
    device.tick();
    let first = device.report_at(&later, 3).unwrap();
    assert!(first.values.is_empty(), "{first:?}");
    assert_eq!(status_of(&first, SUM), None, "not reached");
}
